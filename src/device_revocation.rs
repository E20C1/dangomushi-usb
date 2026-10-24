use crate::trusted_devices;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const STORE_KIND: &str = "dangomushi-device-revocations";
const FORMAT_VERSION: u64 = 1;
const STORE_FILE: &str = "device_revocations.v1.json";
const PREVIOUS_FILE: &str = "device_revocations.v1.previous";
const PENDING_FILE: &str = "device_revocations.v1.pending";

pub const MAX_REVOKED_DEVICE_KEYS: usize = 4096;
pub const MAX_REVOCATION_STORE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationLedger {
    fingerprints: Vec<[u8; 32]>,
}

impl RevocationLedger {
    pub fn fingerprints(&self) -> &[[u8; 32]] {
        &self.fingerprints
    }

    pub fn len(&self) -> usize {
        self.fingerprints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fingerprints.is_empty()
    }

    pub fn contains(&self, fingerprint: &[u8; 32]) -> bool {
        self.fingerprints.binary_search(fingerprint).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokedDevice {
    device_name: String,
    fingerprint: [u8; 32],
}

impl RevokedDevice {
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRevocationError {
    WorkspaceUnavailable,
    UnsafeIdentityLayout,
    InvalidFormat,
    AlreadyRevoked,
    DeviceNotTrusted,
    RevocationLimitExceeded,
    StoreTooLarge,
    VerificationFailed,
    TrustedStateUnavailable,
    Io,
}

impl fmt::Display for DeviceRevocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace identity storage is unavailable",
            Self::UnsafeIdentityLayout => "device-revocation identity layout is unsafe",
            Self::InvalidFormat => "device-revocation ledger format is invalid",
            Self::AlreadyRevoked => "device public key is already revoked",
            Self::DeviceNotTrusted => "device public key is not currently trusted",
            Self::RevocationLimitExceeded => "device-revocation ledger limit was exceeded",
            Self::StoreTooLarge => "device-revocation ledger exceeds its size limit",
            Self::VerificationFailed => "device-revocation replacement verification failed",
            Self::TrustedStateUnavailable => "trusted-device state could not be verified",
            Self::Io => "device-revocation storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for DeviceRevocationError {}

pub fn load(workspace_root: &Path) -> Result<RevocationLedger, DeviceRevocationError> {
    let identity_root = checked_identity_root(workspace_root)?;
    recover_interrupted_write(&identity_root)?;
    load_committed(&identity_root)
}

pub fn revoke(
    workspace_root: &Path,
    fingerprint: &[u8; 32],
) -> Result<RevokedDevice, DeviceRevocationError> {
    let mut ledger = load(workspace_root)?;
    if ledger.contains(fingerprint) {
        return Err(DeviceRevocationError::AlreadyRevoked);
    }

    let trusted = trusted_devices::load(workspace_root)
        .map_err(|_| DeviceRevocationError::TrustedStateUnavailable)?;
    let device = trusted
        .devices()
        .iter()
        .find(|device| device.fingerprint() == fingerprint)
        .cloned()
        .ok_or(DeviceRevocationError::DeviceNotTrusted)?;

    if ledger.fingerprints.len() >= MAX_REVOKED_DEVICE_KEYS {
        return Err(DeviceRevocationError::RevocationLimitExceeded);
    }

    ledger.fingerprints.push(*fingerprint);
    ledger.fingerprints.sort_unstable();
    persist_store(&checked_identity_root(workspace_root)?, &ledger)?;

    Ok(RevokedDevice {
        device_name: device.device_name().to_string(),
        fingerprint: *fingerprint,
    })
}

pub(crate) fn merge_for_workspace_restore(
    workspace_root: &Path,
    staging_root: &Path,
) -> Result<(), DeviceRevocationError> {
    let current_identity = checked_identity_root(workspace_root)?;
    recover_interrupted_write(&current_identity)?;
    let current = load_committed(&current_identity)?;

    let staged_identity = checked_staged_identity_root(staging_root)?;
    recover_interrupted_write(&staged_identity)?;
    let staged = load_committed(&staged_identity)?;

    let mut union = BTreeSet::<[u8; 32]>::new();
    union.extend(current.fingerprints);
    union.extend(staged.fingerprints);

    if union.len() > MAX_REVOKED_DEVICE_KEYS {
        return Err(DeviceRevocationError::RevocationLimitExceeded);
    }

    if union.is_empty() {
        return Ok(());
    }

    let merged = RevocationLedger {
        fingerprints: union.into_iter().collect(),
    };
    persist_store(&staged_identity, &merged)
}

fn checked_identity_root(workspace_root: &Path) -> Result<PathBuf, DeviceRevocationError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| DeviceRevocationError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(DeviceRevocationError::WorkspaceUnavailable);
    }

    let marker = canonical_workspace.join(".dangomushi-workspace");
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| DeviceRevocationError::WorkspaceUnavailable)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    let identity = canonical_workspace.join("identity");
    let metadata =
        fs::symlink_metadata(&identity).map_err(|_| DeviceRevocationError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    let canonical_identity =
        fs::canonicalize(&identity).map_err(|_| DeviceRevocationError::WorkspaceUnavailable)?;
    if !canonical_identity.starts_with(&canonical_workspace) {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    Ok(canonical_identity)
}

fn checked_staged_identity_root(staging_root: &Path) -> Result<PathBuf, DeviceRevocationError> {
    let identity = staging_root.join("identity");
    let metadata =
        fs::symlink_metadata(&identity).map_err(|_| DeviceRevocationError::UnsafeIdentityLayout)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    let canonical_staging =
        fs::canonicalize(staging_root).map_err(|_| DeviceRevocationError::UnsafeIdentityLayout)?;
    let canonical_identity =
        fs::canonicalize(&identity).map_err(|_| DeviceRevocationError::UnsafeIdentityLayout)?;
    if !canonical_identity.starts_with(&canonical_staging) {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    Ok(canonical_identity)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, DeviceRevocationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(DeviceRevocationError::UnsafeIdentityLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(DeviceRevocationError::Io),
    }
}

fn load_committed(identity_root: &Path) -> Result<RevocationLedger, DeviceRevocationError> {
    let target = identity_root.join(STORE_FILE);
    match fixed_file_state(&target)? {
        FixedFileState::Missing => Ok(RevocationLedger::default()),
        FixedFileState::Regular => decode_store(&read_bounded(&target)?),
    }
}

fn recover_interrupted_write(identity_root: &Path) -> Result<(), DeviceRevocationError> {
    let target = identity_root.join(STORE_FILE);
    let previous = identity_root.join(PREVIOUS_FILE);
    let pending = identity_root.join(PENDING_FILE);

    let target_state = fixed_file_state(&target)?;
    let previous_state = fixed_file_state(&previous)?;
    let pending_state = fixed_file_state(&pending)?;

    if previous_state == FixedFileState::Regular {
        if target_state == FixedFileState::Regular {
            if decode_store(&read_bounded(&target)?).is_ok() {
                fs::remove_file(&previous).map_err(|_| DeviceRevocationError::Io)?;
            } else {
                let previous_store = decode_store(&read_bounded(&previous)?)?;
                fs::remove_file(&target).map_err(|_| DeviceRevocationError::Io)?;
                fs::rename(&previous, &target).map_err(|_| DeviceRevocationError::Io)?;
                let restored = decode_store(&read_bounded(&target)?)?;
                if restored != previous_store {
                    return Err(DeviceRevocationError::VerificationFailed);
                }
            }
        } else {
            let previous_store = decode_store(&read_bounded(&previous)?)?;
            fs::rename(&previous, &target).map_err(|_| DeviceRevocationError::Io)?;
            let restored = decode_store(&read_bounded(&target)?)?;
            if restored != previous_store {
                return Err(DeviceRevocationError::VerificationFailed);
            }
        }

        if pending_state == FixedFileState::Regular {
            fs::remove_file(&pending).map_err(|_| DeviceRevocationError::Io)?;
        }
        sync_directory(identity_root)?;
        return Ok(());
    }

    if pending_state == FixedFileState::Regular {
        fs::remove_file(&pending).map_err(|_| DeviceRevocationError::Io)?;
        sync_directory(identity_root)?;
    }

    Ok(())
}

fn persist_store(
    identity_root: &Path,
    ledger: &RevocationLedger,
) -> Result<(), DeviceRevocationError> {
    validate_ledger(ledger)?;
    recover_interrupted_write(identity_root)?;

    let target = identity_root.join(STORE_FILE);
    let previous = identity_root.join(PREVIOUS_FILE);
    let pending = identity_root.join(PENDING_FILE);

    if fixed_file_state(&previous)? != FixedFileState::Missing
        || fixed_file_state(&pending)? != FixedFileState::Missing
    {
        return Err(DeviceRevocationError::UnsafeIdentityLayout);
    }

    let encoded = encode_store(ledger)?;
    write_create_new(&pending, &encoded)?;

    let pending_ledger = decode_store(&read_bounded(&pending)?)?;
    if &pending_ledger != ledger {
        let _ = fs::remove_file(&pending);
        return Err(DeviceRevocationError::VerificationFailed);
    }

    let had_target = fixed_file_state(&target)? == FixedFileState::Regular;
    if had_target {
        fs::rename(&target, &previous).map_err(|_| {
            let _ = fs::remove_file(&pending);
            DeviceRevocationError::Io
        })?;
    }

    if fs::rename(&pending, &target).is_err() {
        if had_target {
            let _ = fs::rename(&previous, &target);
        }
        let _ = fs::remove_file(&pending);
        return Err(DeviceRevocationError::Io);
    }

    sync_directory(identity_root)?;

    let committed = read_bounded(&target).and_then(|bytes| decode_store(&bytes));
    match committed {
        Ok(actual) if &actual == ledger => {
            if had_target {
                fs::remove_file(&previous).map_err(|_| DeviceRevocationError::Io)?;
                sync_directory(identity_root)?;
            }
            Ok(())
        }
        _ => {
            let _ = fs::remove_file(&target);
            if had_target {
                let _ = fs::rename(&previous, &target);
            }
            let _ = sync_directory(identity_root);
            Err(DeviceRevocationError::VerificationFailed)
        }
    }
}

fn validate_ledger(ledger: &RevocationLedger) -> Result<(), DeviceRevocationError> {
    if ledger.fingerprints.len() > MAX_REVOKED_DEVICE_KEYS {
        return Err(DeviceRevocationError::RevocationLimitExceeded);
    }

    for window in ledger.fingerprints.windows(2) {
        if window[0] >= window[1] {
            return Err(DeviceRevocationError::InvalidFormat);
        }
    }

    Ok(())
}

fn encode_store(ledger: &RevocationLedger) -> Result<Vec<u8>, DeviceRevocationError> {
    validate_ledger(ledger)?;

    let revoked = ledger
        .fingerprints
        .iter()
        .map(|fingerprint| hex_upper(fingerprint))
        .collect::<Vec<_>>();

    let value = json!({
        "format": FORMAT_VERSION,
        "kind": STORE_KIND,
        "revoked_fingerprints": revoked,
    });

    let mut encoded =
        serde_json::to_vec_pretty(&value).map_err(|_| DeviceRevocationError::InvalidFormat)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_REVOCATION_STORE_BYTES {
        return Err(DeviceRevocationError::StoreTooLarge);
    }
    Ok(encoded)
}

fn decode_store(bytes: &[u8]) -> Result<RevocationLedger, DeviceRevocationError> {
    if bytes.len() > MAX_REVOCATION_STORE_BYTES {
        return Err(DeviceRevocationError::StoreTooLarge);
    }

    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| DeviceRevocationError::InvalidFormat)?;
    let object = value
        .as_object()
        .ok_or(DeviceRevocationError::InvalidFormat)?;

    if object.len() != 3
        || object.get("format").and_then(Value::as_u64) != Some(FORMAT_VERSION)
        || object.get("kind").and_then(Value::as_str) != Some(STORE_KIND)
    {
        return Err(DeviceRevocationError::InvalidFormat);
    }

    let revoked = object
        .get("revoked_fingerprints")
        .and_then(Value::as_array)
        .ok_or(DeviceRevocationError::InvalidFormat)?;

    if revoked.len() > MAX_REVOKED_DEVICE_KEYS {
        return Err(DeviceRevocationError::RevocationLimitExceeded);
    }

    let mut fingerprints = Vec::with_capacity(revoked.len());
    for fingerprint in revoked {
        let text = fingerprint
            .as_str()
            .ok_or(DeviceRevocationError::InvalidFormat)?;
        fingerprints.push(decode_hex_32(text).ok_or(DeviceRevocationError::InvalidFormat)?);
    }
    fingerprints.sort_unstable();

    let ledger = RevocationLedger { fingerprints };
    validate_ledger(&ledger)?;
    Ok(ledger)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, DeviceRevocationError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(DeviceRevocationError::InvalidFormat);
    }
    let metadata = fs::metadata(path).map_err(|_| DeviceRevocationError::Io)?;
    if metadata.len() > MAX_REVOCATION_STORE_BYTES as u64 {
        return Err(DeviceRevocationError::StoreTooLarge);
    }
    fs::read(path).map_err(|_| DeviceRevocationError::Io)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), DeviceRevocationError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| DeviceRevocationError::Io)?;
    file.write_all(bytes).map_err(|_| DeviceRevocationError::Io)?;
    file.sync_all().map_err(|_| DeviceRevocationError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), DeviceRevocationError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| DeviceRevocationError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), DeviceRevocationError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let result = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .and_then(|directory| directory.sync_all());
    let _ = result;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> Result<(), DeviceRevocationError> {
    Ok(())
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0F) as usize]));
    }
    output
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }

    let bytes = value.as_bytes();
    let mut output = [0u8; 32];
    for index in 0..32 {
        let high = hex_nibble(bytes[index * 2])?;
        let low = hex_nibble(bytes[index * 2 + 1])?;
        output[index] = (high << 4) | low;
    }
    Some(output)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_round_trips_in_sorted_order() {
        let mut ledger = RevocationLedger {
            fingerprints: vec![[9u8; 32], [2u8; 32]],
        };
        ledger.fingerprints.sort_unstable();

        let encoded = encode_store(&ledger).expect("encode");
        let decoded = decode_store(&encoded).expect("decode");
        assert_eq!(decoded, ledger);
    }

    #[test]
    fn duplicate_fingerprints_are_rejected() {
        let ledger = RevocationLedger {
            fingerprints: vec![[7u8; 32], [7u8; 32]],
        };
        assert_eq!(
            validate_ledger(&ledger),
            Err(DeviceRevocationError::InvalidFormat)
        );
    }
}
