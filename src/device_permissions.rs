use crate::trusted_devices;
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const STORE_KIND: &str = "dangomushi-device-permissions";
const FORMAT_VERSION: u64 = 1;
const STORE_FILE: &str = "device_permissions.v1.json";
const PREVIOUS_FILE: &str = "device_permissions.v1.previous";
const PENDING_FILE: &str = "device_permissions.v1.pending";

pub const MAX_PERMISSION_DEVICES: usize = 256;
pub const MAX_PERMISSION_STORE_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionScope {
    StatusRead,
    FilesRead,
    FilesWrite,
    SessionHandoff,
}

impl PermissionScope {
    pub const ALL: [Self; 4] = [
        Self::StatusRead,
        Self::FilesRead,
        Self::FilesWrite,
        Self::SessionHandoff,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StatusRead => "status.read",
            Self::FilesRead => "files.read",
            Self::FilesWrite => "files.write",
            Self::SessionHandoff => "session.handoff",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::StatusRead => "STATUS READ",
            Self::FilesRead => "FILES READ",
            Self::FilesWrite => "FILES WRITE",
            Self::SessionHandoff => "SESSION HANDOFF",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "status.read" => Some(Self::StatusRead),
            "files.read" => Some(Self::FilesRead),
            "files.write" => Some(Self::FilesWrite),
            "session.handoff" => Some(Self::SessionHandoff),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePermissionPolicy {
    fingerprint: [u8; 32],
    scopes: Vec<PermissionScope>,
}

impl DevicePermissionPolicy {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    pub fn scopes(&self) -> &[PermissionScope] {
        &self.scopes
    }

    pub fn allows(&self, scope: PermissionScope) -> bool {
        self.scopes.binary_search(&scope).is_ok()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevicePermissionStore {
    policies: Vec<DevicePermissionPolicy>,
}

impl DevicePermissionStore {
    pub fn policies(&self) -> &[DevicePermissionPolicy] {
        &self.policies
    }

    pub fn policy_for(&self, fingerprint: &[u8; 32]) -> Option<&DevicePermissionPolicy> {
        self.policies
            .binary_search_by_key(fingerprint, |policy| policy.fingerprint)
            .ok()
            .map(|index| &self.policies[index])
    }

    pub fn allows(&self, fingerprint: &[u8; 32], scope: PermissionScope) -> bool {
        self.policy_for(fingerprint)
            .is_some_and(|policy| policy.allows(scope))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePermissionError {
    WorkspaceUnavailable,
    UnsafeStateLayout,
    InvalidFormat,
    DeviceNotTrusted,
    TrustedStateUnavailable,
    DeviceLimitExceeded,
    StoreTooLarge,
    VerificationFailed,
    Io,
}

impl fmt::Display for DevicePermissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace permission storage is unavailable",
            Self::UnsafeStateLayout => "device-permission state layout is unsafe",
            Self::InvalidFormat => "device-permission store format is invalid",
            Self::DeviceNotTrusted => "device public key is not currently trusted",
            Self::TrustedStateUnavailable => "trusted-device state could not be verified",
            Self::DeviceLimitExceeded => "device-permission record limit was exceeded",
            Self::StoreTooLarge => "device-permission store exceeds its size limit",
            Self::VerificationFailed => "device-permission replacement verification failed",
            Self::Io => "device-permission storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for DevicePermissionError {}

pub fn load(workspace_root: &Path) -> Result<DevicePermissionStore, DevicePermissionError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    load_committed(&state_root)
}

pub fn set_scopes(
    workspace_root: &Path,
    fingerprint: &[u8; 32],
    scopes: &[PermissionScope],
) -> Result<DevicePermissionPolicy, DevicePermissionError> {
    let trusted = trusted_devices::load(workspace_root)
        .map_err(|_| DevicePermissionError::TrustedStateUnavailable)?;
    if !trusted
        .devices()
        .iter()
        .any(|device| device.fingerprint() == fingerprint)
    {
        return Err(DevicePermissionError::DeviceNotTrusted);
    }

    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    let mut store = load_committed(&state_root)?;
    let scopes = normalize_scopes(scopes);

    match store
        .policies
        .binary_search_by_key(fingerprint, |policy| policy.fingerprint)
    {
        Ok(index) if scopes.is_empty() => {
            store.policies.remove(index);
        }
        Ok(index) => {
            store.policies[index].scopes = scopes.clone();
        }
        Err(index) if !scopes.is_empty() => {
            if store.policies.len() >= MAX_PERMISSION_DEVICES {
                return Err(DevicePermissionError::DeviceLimitExceeded);
            }
            store.policies.insert(
                index,
                DevicePermissionPolicy {
                    fingerprint: *fingerprint,
                    scopes: scopes.clone(),
                },
            );
        }
        Err(_) => {}
    }

    persist_store(&state_root, &store)?;

    Ok(DevicePermissionPolicy {
        fingerprint: *fingerprint,
        scopes,
    })
}

pub fn authorize(
    workspace_root: &Path,
    fingerprint: &[u8; 32],
    scope: PermissionScope,
) -> Result<bool, DevicePermissionError> {
    let trusted = trusted_devices::load(workspace_root)
        .map_err(|_| DevicePermissionError::TrustedStateUnavailable)?;
    if !trusted
        .devices()
        .iter()
        .any(|device| device.fingerprint() == fingerprint)
    {
        return Ok(false);
    }

    let store = load(workspace_root)?;
    Ok(store.allows(fingerprint, scope))
}

pub(crate) fn reset_for_new_trust(
    workspace_root: &Path,
    fingerprint: &[u8; 32],
) -> Result<(), DevicePermissionError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    let mut store = load_committed(&state_root)?;

    let Ok(index) = store
        .policies
        .binary_search_by_key(fingerprint, |policy| policy.fingerprint)
    else {
        return Ok(());
    };

    store.policies.remove(index);
    persist_store(&state_root, &store)
}

fn normalize_scopes(scopes: &[PermissionScope]) -> Vec<PermissionScope> {
    let mut normalized = scopes.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

fn checked_state_root(workspace_root: &Path) -> Result<PathBuf, DevicePermissionError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| DevicePermissionError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(DevicePermissionError::WorkspaceUnavailable);
    }

    let marker = canonical_workspace.join(".dangomushi-workspace");
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| DevicePermissionError::WorkspaceUnavailable)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(DevicePermissionError::UnsafeStateLayout);
    }

    let state = canonical_workspace.join("state");
    let metadata =
        fs::symlink_metadata(&state).map_err(|_| DevicePermissionError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DevicePermissionError::UnsafeStateLayout);
    }

    let canonical_state =
        fs::canonicalize(&state).map_err(|_| DevicePermissionError::WorkspaceUnavailable)?;
    if !canonical_state.starts_with(&canonical_workspace) {
        return Err(DevicePermissionError::UnsafeStateLayout);
    }

    Ok(canonical_state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, DevicePermissionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(DevicePermissionError::UnsafeStateLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(DevicePermissionError::Io),
    }
}

fn load_committed(state_root: &Path) -> Result<DevicePermissionStore, DevicePermissionError> {
    let target = state_root.join(STORE_FILE);
    match fixed_file_state(&target)? {
        FixedFileState::Missing => Ok(DevicePermissionStore::default()),
        FixedFileState::Regular => decode_store(&read_bounded(&target)?),
    }
}

fn recover_interrupted_write(state_root: &Path) -> Result<(), DevicePermissionError> {
    let target = state_root.join(STORE_FILE);
    let previous = state_root.join(PREVIOUS_FILE);
    let pending = state_root.join(PENDING_FILE);

    let target_state = fixed_file_state(&target)?;
    let previous_state = fixed_file_state(&previous)?;
    let pending_state = fixed_file_state(&pending)?;

    if previous_state == FixedFileState::Regular {
        if target_state == FixedFileState::Regular {
            if decode_store(&read_bounded(&target)?).is_ok() {
                fs::remove_file(&previous).map_err(|_| DevicePermissionError::Io)?;
            } else {
                let previous_store = decode_store(&read_bounded(&previous)?)?;
                fs::remove_file(&target).map_err(|_| DevicePermissionError::Io)?;
                fs::rename(&previous, &target).map_err(|_| DevicePermissionError::Io)?;
                let restored = decode_store(&read_bounded(&target)?)?;
                if restored != previous_store {
                    return Err(DevicePermissionError::VerificationFailed);
                }
            }
        } else {
            let previous_store = decode_store(&read_bounded(&previous)?)?;
            fs::rename(&previous, &target).map_err(|_| DevicePermissionError::Io)?;
            let restored = decode_store(&read_bounded(&target)?)?;
            if restored != previous_store {
                return Err(DevicePermissionError::VerificationFailed);
            }
        }

        if pending_state == FixedFileState::Regular {
            fs::remove_file(&pending).map_err(|_| DevicePermissionError::Io)?;
        }
        sync_directory(state_root)?;
        return Ok(());
    }

    if pending_state == FixedFileState::Regular {
        fs::remove_file(&pending).map_err(|_| DevicePermissionError::Io)?;
        sync_directory(state_root)?;
    }

    Ok(())
}

fn persist_store(
    state_root: &Path,
    store: &DevicePermissionStore,
) -> Result<(), DevicePermissionError> {
    validate_store(store)?;
    recover_interrupted_write(state_root)?;

    let target = state_root.join(STORE_FILE);
    let previous = state_root.join(PREVIOUS_FILE);
    let pending = state_root.join(PENDING_FILE);

    if fixed_file_state(&previous)? != FixedFileState::Missing
        || fixed_file_state(&pending)? != FixedFileState::Missing
    {
        return Err(DevicePermissionError::UnsafeStateLayout);
    }

    let encoded = encode_store(store)?;
    write_create_new(&pending, &encoded)?;

    let pending_store = decode_store(&read_bounded(&pending)?)?;
    if &pending_store != store {
        let _ = fs::remove_file(&pending);
        return Err(DevicePermissionError::VerificationFailed);
    }

    let had_target = fixed_file_state(&target)? == FixedFileState::Regular;
    if had_target {
        fs::rename(&target, &previous).map_err(|_| {
            let _ = fs::remove_file(&pending);
            DevicePermissionError::Io
        })?;
    }

    if fs::rename(&pending, &target).is_err() {
        if had_target {
            let _ = fs::rename(&previous, &target);
        }
        let _ = fs::remove_file(&pending);
        return Err(DevicePermissionError::Io);
    }

    sync_directory(state_root)?;

    let committed = read_bounded(&target).and_then(|bytes| decode_store(&bytes));
    match committed {
        Ok(actual) if &actual == store => {
            if had_target {
                fs::remove_file(&previous).map_err(|_| DevicePermissionError::Io)?;
                sync_directory(state_root)?;
            }
            Ok(())
        }
        _ => {
            let _ = fs::remove_file(&target);
            if had_target {
                let _ = fs::rename(&previous, &target);
            }
            let _ = sync_directory(state_root);
            Err(DevicePermissionError::VerificationFailed)
        }
    }
}

fn validate_store(store: &DevicePermissionStore) -> Result<(), DevicePermissionError> {
    if store.policies.len() > MAX_PERMISSION_DEVICES {
        return Err(DevicePermissionError::DeviceLimitExceeded);
    }

    for window in store.policies.windows(2) {
        if window[0].fingerprint >= window[1].fingerprint {
            return Err(DevicePermissionError::InvalidFormat);
        }
    }

    for policy in &store.policies {
        if policy.scopes.is_empty() {
            return Err(DevicePermissionError::InvalidFormat);
        }
        for window in policy.scopes.windows(2) {
            if window[0] >= window[1] {
                return Err(DevicePermissionError::InvalidFormat);
            }
        }
    }

    Ok(())
}

fn encode_store(store: &DevicePermissionStore) -> Result<Vec<u8>, DevicePermissionError> {
    validate_store(store)?;

    let devices = store
        .policies
        .iter()
        .map(|policy| {
            json!({
                "fingerprint": hex_upper(&policy.fingerprint),
                "scopes": policy
                    .scopes
                    .iter()
                    .map(|scope| scope.as_str())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let value = json!({
        "format": FORMAT_VERSION,
        "kind": STORE_KIND,
        "devices": devices,
    });

    let mut encoded =
        serde_json::to_vec_pretty(&value).map_err(|_| DevicePermissionError::InvalidFormat)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_PERMISSION_STORE_BYTES {
        return Err(DevicePermissionError::StoreTooLarge);
    }
    Ok(encoded)
}

fn decode_store(bytes: &[u8]) -> Result<DevicePermissionStore, DevicePermissionError> {
    if bytes.len() > MAX_PERMISSION_STORE_BYTES {
        return Err(DevicePermissionError::StoreTooLarge);
    }

    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| DevicePermissionError::InvalidFormat)?;
    let object = value
        .as_object()
        .ok_or(DevicePermissionError::InvalidFormat)?;

    if object.len() != 3
        || object.get("format").and_then(Value::as_u64) != Some(FORMAT_VERSION)
        || object.get("kind").and_then(Value::as_str) != Some(STORE_KIND)
    {
        return Err(DevicePermissionError::InvalidFormat);
    }

    let records = object
        .get("devices")
        .and_then(Value::as_array)
        .ok_or(DevicePermissionError::InvalidFormat)?;
    if records.len() > MAX_PERMISSION_DEVICES {
        return Err(DevicePermissionError::DeviceLimitExceeded);
    }

    let mut policies = Vec::with_capacity(records.len());
    for record in records {
        let record = record
            .as_object()
            .ok_or(DevicePermissionError::InvalidFormat)?;
        if record.len() != 2 {
            return Err(DevicePermissionError::InvalidFormat);
        }

        let fingerprint = record
            .get("fingerprint")
            .and_then(Value::as_str)
            .and_then(decode_hex_32)
            .ok_or(DevicePermissionError::InvalidFormat)?;

        let raw_scopes = record
            .get("scopes")
            .and_then(Value::as_array)
            .ok_or(DevicePermissionError::InvalidFormat)?;
        if raw_scopes.is_empty() || raw_scopes.len() > PermissionScope::ALL.len() {
            return Err(DevicePermissionError::InvalidFormat);
        }

        let mut scopes = Vec::with_capacity(raw_scopes.len());
        for raw_scope in raw_scopes {
            let scope = raw_scope
                .as_str()
                .and_then(PermissionScope::from_str)
                .ok_or(DevicePermissionError::InvalidFormat)?;
            scopes.push(scope);
        }
        scopes.sort_unstable();

        policies.push(DevicePermissionPolicy {
            fingerprint,
            scopes,
        });
    }

    policies.sort_unstable_by_key(|policy| policy.fingerprint);
    let store = DevicePermissionStore { policies };
    validate_store(&store)?;
    Ok(store)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, DevicePermissionError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(DevicePermissionError::InvalidFormat);
    }
    let metadata = fs::metadata(path).map_err(|_| DevicePermissionError::Io)?;
    if metadata.len() > MAX_PERMISSION_STORE_BYTES as u64 {
        return Err(DevicePermissionError::StoreTooLarge);
    }
    fs::read(path).map_err(|_| DevicePermissionError::Io)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), DevicePermissionError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| DevicePermissionError::Io)?;
    file.write_all(bytes).map_err(|_| DevicePermissionError::Io)?;
    file.sync_all().map_err(|_| DevicePermissionError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), DevicePermissionError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| DevicePermissionError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), DevicePermissionError> {
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
fn sync_directory(_path: &Path) -> Result<(), DevicePermissionError> {
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
    fn scope_identifiers_are_stable_and_unique() {
        let names = PermissionScope::ALL.map(PermissionScope::as_str);
        assert_eq!(
            names,
            ["status.read", "files.read", "files.write", "session.handoff"]
        );
    }

    #[test]
    fn duplicate_input_scopes_are_normalized() {
        assert_eq!(
            normalize_scopes(&[
                PermissionScope::FilesRead,
                PermissionScope::StatusRead,
                PermissionScope::FilesRead,
            ]),
            vec![PermissionScope::StatusRead, PermissionScope::FilesRead]
        );
    }
}
