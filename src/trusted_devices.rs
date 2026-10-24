use crate::device_pairing::{
    device_key_fingerprint, validate_device_name, PairedDeviceCandidate, PAIRING_PUBLIC_KEY_BYTES,
};
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const STORE_KIND: &str = "dangomushi-trusted-devices";
const FORMAT_VERSION: u64 = 1;
const STORE_FILE: &str = "trusted_devices.v1.json";
const PREVIOUS_FILE: &str = "trusted_devices.v1.previous";
const PENDING_FILE: &str = "trusted_devices.v1.pending";

pub const MAX_TRUSTED_DEVICES: usize = 256;
pub const MAX_TRUST_STORE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDevice {
    device_name: String,
    public_key: [u8; PAIRING_PUBLIC_KEY_BYTES],
    fingerprint: [u8; 32],
}

impl TrustedDevice {
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub const fn public_key(&self) -> &[u8; PAIRING_PUBLIC_KEY_BYTES] {
        &self.public_key
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    fn from_candidate(candidate: &PairedDeviceCandidate) -> Self {
        Self {
            device_name: candidate.device_name().to_string(),
            public_key: *candidate.public_key(),
            fingerprint: *candidate.fingerprint(),
        }
    }

    fn validate(&self) -> Result<(), TrustedDeviceError> {
        validate_device_name(&self.device_name).map_err(|_| TrustedDeviceError::InvalidFormat)?;
        if device_key_fingerprint(&self.public_key) != self.fingerprint {
            return Err(TrustedDeviceError::FingerprintMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedDeviceStore {
    devices: Vec<TrustedDevice>,
}

impl TrustedDeviceStore {
    pub fn devices(&self) -> &[TrustedDevice] {
        &self.devices
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedDeviceError {
    WorkspaceUnavailable,
    UnsafeIdentityLayout,
    InvalidFormat,
    FingerprintMismatch,
    AlreadyTrusted,
    DeviceLimitExceeded,
    StoreTooLarge,
    VerificationFailed,
    Io,
}

impl fmt::Display for TrustedDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace identity storage is unavailable",
            Self::UnsafeIdentityLayout => "trusted-device identity layout is unsafe",
            Self::InvalidFormat => "trusted-device store format is invalid",
            Self::FingerprintMismatch => "trusted-device fingerprint does not match its public key",
            Self::AlreadyTrusted => "device public key is already trusted",
            Self::DeviceLimitExceeded => "trusted-device limit was exceeded",
            Self::StoreTooLarge => "trusted-device store exceeds its size limit",
            Self::VerificationFailed => "trusted-device replacement verification failed",
            Self::Io => "trusted-device storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for TrustedDeviceError {}

pub fn load(workspace_root: &Path) -> Result<TrustedDeviceStore, TrustedDeviceError> {
    let identity_root = checked_identity_root(workspace_root)?;
    recover_interrupted_write(&identity_root)?;
    load_committed(&identity_root)
}

pub fn register_candidate(
    workspace_root: &Path,
    candidate: &PairedDeviceCandidate,
) -> Result<TrustedDevice, TrustedDeviceError> {
    let identity_root = checked_identity_root(workspace_root)?;
    recover_interrupted_write(&identity_root)?;

    let mut store = load_committed(&identity_root)?;
    let trusted = TrustedDevice::from_candidate(candidate);
    trusted.validate()?;

    if store.devices.iter().any(|existing| {
        existing.public_key == trusted.public_key || existing.fingerprint == trusted.fingerprint
    }) {
        return Err(TrustedDeviceError::AlreadyTrusted);
    }

    if store.devices.len() >= MAX_TRUSTED_DEVICES {
        return Err(TrustedDeviceError::DeviceLimitExceeded);
    }

    store.devices.push(trusted.clone());
    persist_store(&identity_root, &store)?;
    Ok(trusted)
}

fn checked_identity_root(workspace_root: &Path) -> Result<PathBuf, TrustedDeviceError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| TrustedDeviceError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(TrustedDeviceError::WorkspaceUnavailable);
    }

    let marker = canonical_workspace.join(".dangomushi-workspace");
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| TrustedDeviceError::WorkspaceUnavailable)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(TrustedDeviceError::UnsafeIdentityLayout);
    }

    let identity = canonical_workspace.join("identity");
    let metadata =
        fs::symlink_metadata(&identity).map_err(|_| TrustedDeviceError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TrustedDeviceError::UnsafeIdentityLayout);
    }

    let canonical_identity =
        fs::canonicalize(&identity).map_err(|_| TrustedDeviceError::WorkspaceUnavailable)?;
    if !canonical_identity.starts_with(&canonical_workspace) {
        return Err(TrustedDeviceError::UnsafeIdentityLayout);
    }

    Ok(canonical_identity)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, TrustedDeviceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TrustedDeviceError::UnsafeIdentityLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(TrustedDeviceError::Io),
    }
}

fn load_committed(identity_root: &Path) -> Result<TrustedDeviceStore, TrustedDeviceError> {
    let target = identity_root.join(STORE_FILE);
    match fixed_file_state(&target)? {
        FixedFileState::Missing => Ok(TrustedDeviceStore::default()),
        FixedFileState::Regular => decode_store(&read_bounded(&target)?),
    }
}

fn recover_interrupted_write(identity_root: &Path) -> Result<(), TrustedDeviceError> {
    let target = identity_root.join(STORE_FILE);
    let previous = identity_root.join(PREVIOUS_FILE);
    let pending = identity_root.join(PENDING_FILE);

    let target_state = fixed_file_state(&target)?;
    let previous_state = fixed_file_state(&previous)?;
    let pending_state = fixed_file_state(&pending)?;

    if previous_state == FixedFileState::Regular {
        if target_state == FixedFileState::Regular {
            if decode_store(&read_bounded(&target)?).is_ok() {
                fs::remove_file(&previous).map_err(|_| TrustedDeviceError::Io)?;
            } else {
                let previous_store = decode_store(&read_bounded(&previous)?)?;
                fs::remove_file(&target).map_err(|_| TrustedDeviceError::Io)?;
                fs::rename(&previous, &target).map_err(|_| TrustedDeviceError::Io)?;
                let restored = decode_store(&read_bounded(&target)?)?;
                if restored != previous_store {
                    return Err(TrustedDeviceError::VerificationFailed);
                }
            }
        } else {
            let previous_store = decode_store(&read_bounded(&previous)?)?;
            fs::rename(&previous, &target).map_err(|_| TrustedDeviceError::Io)?;
            let restored = decode_store(&read_bounded(&target)?)?;
            if restored != previous_store {
                return Err(TrustedDeviceError::VerificationFailed);
            }
        }

        if pending_state == FixedFileState::Regular {
            fs::remove_file(&pending).map_err(|_| TrustedDeviceError::Io)?;
        }
        sync_directory(identity_root)?;
        return Ok(());
    }

    if pending_state == FixedFileState::Regular {
        fs::remove_file(&pending).map_err(|_| TrustedDeviceError::Io)?;
        sync_directory(identity_root)?;
    }

    Ok(())
}

fn persist_store(
    identity_root: &Path,
    store: &TrustedDeviceStore,
) -> Result<(), TrustedDeviceError> {
    validate_store(store)?;
    recover_interrupted_write(identity_root)?;

    let target = identity_root.join(STORE_FILE);
    let previous = identity_root.join(PREVIOUS_FILE);
    let pending = identity_root.join(PENDING_FILE);

    if fixed_file_state(&previous)? != FixedFileState::Missing
        || fixed_file_state(&pending)? != FixedFileState::Missing
    {
        return Err(TrustedDeviceError::UnsafeIdentityLayout);
    }

    let encoded = encode_store(store)?;
    write_create_new(&pending, &encoded)?;

    let pending_store = decode_store(&read_bounded(&pending)?)?;
    if &pending_store != store {
        let _ = fs::remove_file(&pending);
        return Err(TrustedDeviceError::VerificationFailed);
    }

    let had_target = fixed_file_state(&target)? == FixedFileState::Regular;
    if had_target {
        fs::rename(&target, &previous).map_err(|_| {
            let _ = fs::remove_file(&pending);
            TrustedDeviceError::Io
        })?;
    }

    if fs::rename(&pending, &target).is_err() {
        if had_target {
            let _ = fs::rename(&previous, &target);
        }
        let _ = fs::remove_file(&pending);
        return Err(TrustedDeviceError::Io);
    }

    sync_directory(identity_root)?;

    let committed = read_bounded(&target).and_then(|bytes| decode_store(&bytes));
    match committed {
        Ok(actual) if &actual == store => {
            if had_target {
                fs::remove_file(&previous).map_err(|_| TrustedDeviceError::Io)?;
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
            Err(TrustedDeviceError::VerificationFailed)
        }
    }
}

fn validate_store(store: &TrustedDeviceStore) -> Result<(), TrustedDeviceError> {
    if store.devices.len() > MAX_TRUSTED_DEVICES {
        return Err(TrustedDeviceError::DeviceLimitExceeded);
    }

    for (index, device) in store.devices.iter().enumerate() {
        device.validate()?;
        if store.devices[..index].iter().any(|existing| {
            existing.public_key == device.public_key
                || existing.fingerprint == device.fingerprint
        }) {
            return Err(TrustedDeviceError::InvalidFormat);
        }
    }

    Ok(())
}

fn encode_store(store: &TrustedDeviceStore) -> Result<Vec<u8>, TrustedDeviceError> {
    validate_store(store)?;

    let devices = store
        .devices
        .iter()
        .map(|device| {
            json!({
                "name": device.device_name(),
                "public_key": hex_upper(&device.public_key),
                "fingerprint": hex_upper(&device.fingerprint),
            })
        })
        .collect::<Vec<_>>();

    let value = json!({
        "format": FORMAT_VERSION,
        "kind": STORE_KIND,
        "devices": devices,
    });

    let mut encoded =
        serde_json::to_vec_pretty(&value).map_err(|_| TrustedDeviceError::InvalidFormat)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_TRUST_STORE_BYTES {
        return Err(TrustedDeviceError::StoreTooLarge);
    }
    Ok(encoded)
}

fn decode_store(bytes: &[u8]) -> Result<TrustedDeviceStore, TrustedDeviceError> {
    if bytes.len() > MAX_TRUST_STORE_BYTES {
        return Err(TrustedDeviceError::StoreTooLarge);
    }

    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| TrustedDeviceError::InvalidFormat)?;
    let object = value.as_object().ok_or(TrustedDeviceError::InvalidFormat)?;

    if object.len() != 3
        || object.get("format").and_then(Value::as_u64) != Some(FORMAT_VERSION)
        || object.get("kind").and_then(Value::as_str) != Some(STORE_KIND)
    {
        return Err(TrustedDeviceError::InvalidFormat);
    }

    let records = object
        .get("devices")
        .and_then(Value::as_array)
        .ok_or(TrustedDeviceError::InvalidFormat)?;

    if records.len() > MAX_TRUSTED_DEVICES {
        return Err(TrustedDeviceError::DeviceLimitExceeded);
    }

    let mut devices = Vec::with_capacity(records.len());
    for record in records {
        let record = record.as_object().ok_or(TrustedDeviceError::InvalidFormat)?;
        if record.len() != 3 {
            return Err(TrustedDeviceError::InvalidFormat);
        }

        let device_name = record
            .get("name")
            .and_then(Value::as_str)
            .ok_or(TrustedDeviceError::InvalidFormat)?
            .to_string();
        validate_device_name(&device_name).map_err(|_| TrustedDeviceError::InvalidFormat)?;

        let public_key = decode_hex_32(
            record
                .get("public_key")
                .and_then(Value::as_str)
                .ok_or(TrustedDeviceError::InvalidFormat)?,
        )
        .ok_or(TrustedDeviceError::InvalidFormat)?;

        let fingerprint = decode_hex_32(
            record
                .get("fingerprint")
                .and_then(Value::as_str)
                .ok_or(TrustedDeviceError::InvalidFormat)?,
        )
        .ok_or(TrustedDeviceError::InvalidFormat)?;

        let device = TrustedDevice {
            device_name,
            public_key,
            fingerprint,
        };
        device.validate()?;
        devices.push(device);
    }

    let store = TrustedDeviceStore { devices };
    validate_store(&store)?;
    Ok(store)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, TrustedDeviceError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(TrustedDeviceError::InvalidFormat);
    }
    let metadata = fs::metadata(path).map_err(|_| TrustedDeviceError::Io)?;
    if metadata.len() > MAX_TRUST_STORE_BYTES as u64 {
        return Err(TrustedDeviceError::StoreTooLarge);
    }
    fs::read(path).map_err(|_| TrustedDeviceError::Io)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), TrustedDeviceError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| TrustedDeviceError::Io)?;
    file.write_all(bytes).map_err(|_| TrustedDeviceError::Io)?;
    file.sync_all().map_err(|_| TrustedDeviceError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), TrustedDeviceError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| TrustedDeviceError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), TrustedDeviceError> {
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
fn sync_directory(_path: &Path) -> Result<(), TrustedDeviceError> {
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

    fn test_device(seed: u8, name: &str) -> TrustedDevice {
        let public_key = [seed; PAIRING_PUBLIC_KEY_BYTES];
        TrustedDevice {
            device_name: name.to_string(),
            public_key,
            fingerprint: device_key_fingerprint(&public_key),
        }
    }

    #[test]
    fn store_round_trips_and_verifies_fingerprints() {
        let store = TrustedDeviceStore {
            devices: vec![test_device(7, "Pixel")],
        };
        let encoded = encode_store(&store).expect("encode");
        let decoded = decode_store(&encoded).expect("decode");
        assert_eq!(decoded, store);
    }

    #[test]
    fn duplicate_public_keys_are_rejected() {
        let device = test_device(9, "Phone");
        let store = TrustedDeviceStore {
            devices: vec![device.clone(), device],
        };
        assert_eq!(
            validate_store(&store),
            Err(TrustedDeviceError::InvalidFormat)
        );
    }

    #[test]
    fn fingerprint_mismatch_is_rejected() {
        let mut device = test_device(3, "Tablet");
        device.fingerprint[0] ^= 0xFF;
        assert_eq!(
            device.validate(),
            Err(TrustedDeviceError::FingerprintMismatch)
        );
    }
}
