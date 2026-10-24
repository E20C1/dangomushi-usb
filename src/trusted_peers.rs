use crate::peer_identity::{
    self, VerifiedPeerCandidate, PEER_FINGERPRINT_BYTES, PEER_PUBLIC_KEY_BYTES,
};
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const STORE_KIND: &str = "dangomushi-trusted-peers";
const FORMAT_VERSION: u64 = 1;
const STORE_FILE: &str = "trusted_peers.v1.json";
const PREVIOUS_FILE: &str = "trusted_peers.v1.previous";
const PENDING_FILE: &str = "trusted_peers.v1.pending";

pub const MAX_TRUSTED_PEERS: usize = 256;
pub const MAX_TRUSTED_PEER_STORE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPeer {
    peer_name: String,
    public_key: [u8; PEER_PUBLIC_KEY_BYTES],
    fingerprint: [u8; PEER_FINGERPRINT_BYTES],
}

impl TrustedPeer {
    pub fn peer_name(&self) -> &str {
        &self.peer_name
    }

    pub const fn public_key(&self) -> &[u8; PEER_PUBLIC_KEY_BYTES] {
        &self.public_key
    }

    pub const fn fingerprint(&self) -> &[u8; PEER_FINGERPRINT_BYTES] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    fn from_verified(candidate: &VerifiedPeerCandidate) -> Self {
        Self {
            peer_name: candidate.peer_name().to_string(),
            public_key: *candidate.public_key(),
            fingerprint: *candidate.fingerprint(),
        }
    }

    fn validate(&self) -> Result<(), TrustedPeerError> {
        peer_identity::validate_peer_name(&self.peer_name)
            .map_err(|_| TrustedPeerError::InvalidFormat)?;
        if peer_identity::peer_fingerprint(&self.public_key) != self.fingerprint {
            return Err(TrustedPeerError::FingerprintMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedPeerStore {
    peers: Vec<TrustedPeer>,
}

impl TrustedPeerStore {
    pub fn peers(&self) -> &[TrustedPeer] {
        &self.peers
    }

    pub fn peer_for(&self, fingerprint: &[u8; PEER_FINGERPRINT_BYTES]) -> Option<&TrustedPeer> {
        self.peers
            .binary_search_by_key(fingerprint, |peer| peer.fingerprint)
            .ok()
            .map(|index| &self.peers[index])
    }

    pub fn contains(&self, fingerprint: &[u8; PEER_FINGERPRINT_BYTES]) -> bool {
        self.peer_for(fingerprint).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedPeerError {
    WorkspaceUnavailable,
    UnsafeStateLayout,
    InvalidFormat,
    FingerprintMismatch,
    AlreadyTrusted,
    PeerNotTrusted,
    PeerLimitExceeded,
    StoreTooLarge,
    VerificationFailed,
    Io,
}

impl fmt::Display for TrustedPeerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace trusted-peer storage is unavailable",
            Self::UnsafeStateLayout => "trusted-peer state layout is unsafe",
            Self::InvalidFormat => "trusted-peer store format is invalid",
            Self::FingerprintMismatch => "trusted-peer fingerprint does not match its public key",
            Self::AlreadyTrusted => "peer public key is already trusted",
            Self::PeerNotTrusted => "peer public key is not currently trusted",
            Self::PeerLimitExceeded => "trusted-peer limit was exceeded",
            Self::StoreTooLarge => "trusted-peer store exceeds its size limit",
            Self::VerificationFailed => "trusted-peer replacement verification failed",
            Self::Io => "trusted-peer storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for TrustedPeerError {}

pub fn load(workspace_root: &Path) -> Result<TrustedPeerStore, TrustedPeerError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    load_committed(&state_root)
}

pub fn trust_verified_peer(
    workspace_root: &Path,
    candidate: &VerifiedPeerCandidate,
) -> Result<TrustedPeer, TrustedPeerError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    let mut store = load_committed(&state_root)?;

    let trusted = TrustedPeer::from_verified(candidate);
    trusted.validate()?;

    if store.peers.iter().any(|existing| {
        existing.public_key == trusted.public_key || existing.fingerprint == trusted.fingerprint
    }) {
        return Err(TrustedPeerError::AlreadyTrusted);
    }

    if store.peers.len() >= MAX_TRUSTED_PEERS {
        return Err(TrustedPeerError::PeerLimitExceeded);
    }

    store.peers.push(trusted.clone());
    store.peers.sort_unstable_by_key(|peer| peer.fingerprint);
    persist_store(&state_root, &store)?;
    Ok(trusted)
}

pub fn remove_trusted_peer(
    workspace_root: &Path,
    fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
) -> Result<TrustedPeer, TrustedPeerError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_write(&state_root)?;
    let mut store = load_committed(&state_root)?;

    let index = store
        .peers
        .binary_search_by_key(fingerprint, |peer| peer.fingerprint)
        .map_err(|_| TrustedPeerError::PeerNotTrusted)?;
    let removed = store.peers.remove(index);
    persist_store(&state_root, &store)?;
    Ok(removed)
}

fn checked_state_root(workspace_root: &Path) -> Result<PathBuf, TrustedPeerError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| TrustedPeerError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(TrustedPeerError::WorkspaceUnavailable);
    }

    let marker = canonical_workspace.join(".dangomushi-workspace");
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| TrustedPeerError::WorkspaceUnavailable)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(TrustedPeerError::UnsafeStateLayout);
    }

    let state = canonical_workspace.join("state");
    let metadata =
        fs::symlink_metadata(&state).map_err(|_| TrustedPeerError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TrustedPeerError::UnsafeStateLayout);
    }

    let canonical_state =
        fs::canonicalize(&state).map_err(|_| TrustedPeerError::WorkspaceUnavailable)?;
    if !canonical_state.starts_with(&canonical_workspace) {
        return Err(TrustedPeerError::UnsafeStateLayout);
    }

    Ok(canonical_state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, TrustedPeerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TrustedPeerError::UnsafeStateLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(TrustedPeerError::Io),
    }
}

fn load_committed(state_root: &Path) -> Result<TrustedPeerStore, TrustedPeerError> {
    let target = state_root.join(STORE_FILE);
    match fixed_file_state(&target)? {
        FixedFileState::Missing => Ok(TrustedPeerStore::default()),
        FixedFileState::Regular => decode_store(&read_bounded(&target)?),
    }
}

fn recover_interrupted_write(state_root: &Path) -> Result<(), TrustedPeerError> {
    let target = state_root.join(STORE_FILE);
    let previous = state_root.join(PREVIOUS_FILE);
    let pending = state_root.join(PENDING_FILE);

    let target_state = fixed_file_state(&target)?;
    let previous_state = fixed_file_state(&previous)?;
    let pending_state = fixed_file_state(&pending)?;

    if previous_state == FixedFileState::Regular {
        if target_state == FixedFileState::Regular {
            if decode_store(&read_bounded(&target)?).is_ok() {
                fs::remove_file(&previous).map_err(|_| TrustedPeerError::Io)?;
            } else {
                let previous_store = decode_store(&read_bounded(&previous)?)?;
                fs::remove_file(&target).map_err(|_| TrustedPeerError::Io)?;
                fs::rename(&previous, &target).map_err(|_| TrustedPeerError::Io)?;
                let restored = decode_store(&read_bounded(&target)?)?;
                if restored != previous_store {
                    return Err(TrustedPeerError::VerificationFailed);
                }
            }
        } else {
            let previous_store = decode_store(&read_bounded(&previous)?)?;
            fs::rename(&previous, &target).map_err(|_| TrustedPeerError::Io)?;
            let restored = decode_store(&read_bounded(&target)?)?;
            if restored != previous_store {
                return Err(TrustedPeerError::VerificationFailed);
            }
        }

        if pending_state == FixedFileState::Regular {
            fs::remove_file(&pending).map_err(|_| TrustedPeerError::Io)?;
        }
        sync_directory(state_root)?;
        return Ok(());
    }

    if pending_state == FixedFileState::Regular {
        fs::remove_file(&pending).map_err(|_| TrustedPeerError::Io)?;
        sync_directory(state_root)?;
    }

    Ok(())
}

fn persist_store(
    state_root: &Path,
    store: &TrustedPeerStore,
) -> Result<(), TrustedPeerError> {
    validate_store(store)?;
    recover_interrupted_write(state_root)?;

    let target = state_root.join(STORE_FILE);
    let previous = state_root.join(PREVIOUS_FILE);
    let pending = state_root.join(PENDING_FILE);

    if fixed_file_state(&previous)? != FixedFileState::Missing
        || fixed_file_state(&pending)? != FixedFileState::Missing
    {
        return Err(TrustedPeerError::UnsafeStateLayout);
    }

    let encoded = encode_store(store)?;
    write_create_new(&pending, &encoded)?;

    let pending_store = decode_store(&read_bounded(&pending)?)?;
    if &pending_store != store {
        let _ = fs::remove_file(&pending);
        return Err(TrustedPeerError::VerificationFailed);
    }

    let had_target = fixed_file_state(&target)? == FixedFileState::Regular;
    if had_target {
        fs::rename(&target, &previous).map_err(|_| {
            let _ = fs::remove_file(&pending);
            TrustedPeerError::Io
        })?;
    }

    if fs::rename(&pending, &target).is_err() {
        if had_target {
            let _ = fs::rename(&previous, &target);
        }
        let _ = fs::remove_file(&pending);
        return Err(TrustedPeerError::Io);
    }

    sync_directory(state_root)?;

    let committed = read_bounded(&target).and_then(|bytes| decode_store(&bytes));
    match committed {
        Ok(actual) if &actual == store => {
            if had_target {
                fs::remove_file(&previous).map_err(|_| TrustedPeerError::Io)?;
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
            Err(TrustedPeerError::VerificationFailed)
        }
    }
}

fn validate_store(store: &TrustedPeerStore) -> Result<(), TrustedPeerError> {
    if store.peers.len() > MAX_TRUSTED_PEERS {
        return Err(TrustedPeerError::PeerLimitExceeded);
    }

    for peer in &store.peers {
        peer.validate()?;
    }

    for window in store.peers.windows(2) {
        if window[0].fingerprint >= window[1].fingerprint
            || window[0].public_key == window[1].public_key
        {
            return Err(TrustedPeerError::InvalidFormat);
        }
    }

    Ok(())
}

fn encode_store(store: &TrustedPeerStore) -> Result<Vec<u8>, TrustedPeerError> {
    validate_store(store)?;

    let peers = store
        .peers
        .iter()
        .map(|peer| {
            json!({
                "name": peer.peer_name(),
                "public_key": hex_upper(peer.public_key()),
                "fingerprint": hex_upper(peer.fingerprint()),
            })
        })
        .collect::<Vec<_>>();

    let value = json!({
        "format": FORMAT_VERSION,
        "kind": STORE_KIND,
        "peers": peers,
    });

    let mut encoded =
        serde_json::to_vec_pretty(&value).map_err(|_| TrustedPeerError::InvalidFormat)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_TRUSTED_PEER_STORE_BYTES {
        return Err(TrustedPeerError::StoreTooLarge);
    }
    Ok(encoded)
}

fn decode_store(bytes: &[u8]) -> Result<TrustedPeerStore, TrustedPeerError> {
    if bytes.len() > MAX_TRUSTED_PEER_STORE_BYTES {
        return Err(TrustedPeerError::StoreTooLarge);
    }

    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| TrustedPeerError::InvalidFormat)?;
    let object = value.as_object().ok_or(TrustedPeerError::InvalidFormat)?;

    if object.len() != 3
        || object.get("format").and_then(Value::as_u64) != Some(FORMAT_VERSION)
        || object.get("kind").and_then(Value::as_str) != Some(STORE_KIND)
    {
        return Err(TrustedPeerError::InvalidFormat);
    }

    let records = object
        .get("peers")
        .and_then(Value::as_array)
        .ok_or(TrustedPeerError::InvalidFormat)?;
    if records.len() > MAX_TRUSTED_PEERS {
        return Err(TrustedPeerError::PeerLimitExceeded);
    }

    let mut peers = Vec::with_capacity(records.len());
    for record in records {
        let record = record
            .as_object()
            .ok_or(TrustedPeerError::InvalidFormat)?;
        if record.len() != 3 {
            return Err(TrustedPeerError::InvalidFormat);
        }

        let peer_name = record
            .get("name")
            .and_then(Value::as_str)
            .ok_or(TrustedPeerError::InvalidFormat)?
            .to_string();
        peer_identity::validate_peer_name(&peer_name)
            .map_err(|_| TrustedPeerError::InvalidFormat)?;

        let public_key = decode_hex_array::<PEER_PUBLIC_KEY_BYTES>(
            record
                .get("public_key")
                .and_then(Value::as_str)
                .ok_or(TrustedPeerError::InvalidFormat)?,
        )?;
        let fingerprint = decode_hex_array::<PEER_FINGERPRINT_BYTES>(
            record
                .get("fingerprint")
                .and_then(Value::as_str)
                .ok_or(TrustedPeerError::InvalidFormat)?,
        )?;

        peers.push(TrustedPeer {
            peer_name,
            public_key,
            fingerprint,
        });
    }

    peers.sort_unstable_by_key(|peer| peer.fingerprint);
    let store = TrustedPeerStore { peers };
    validate_store(&store)?;
    Ok(store)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, TrustedPeerError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(TrustedPeerError::InvalidFormat);
    }
    let metadata = fs::metadata(path).map_err(|_| TrustedPeerError::Io)?;
    if metadata.len() > MAX_TRUSTED_PEER_STORE_BYTES as u64 {
        return Err(TrustedPeerError::StoreTooLarge);
    }
    fs::read(path).map_err(|_| TrustedPeerError::Io)
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), TrustedPeerError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| TrustedPeerError::Io)?;
    file.write_all(bytes).map_err(|_| TrustedPeerError::Io)?;
    file.sync_all().map_err(|_| TrustedPeerError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), TrustedPeerError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| TrustedPeerError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), TrustedPeerError> {
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
fn sync_directory(_path: &Path) -> Result<(), TrustedPeerError> {
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

fn decode_hex_array<const N: usize>(value: &str) -> Result<[u8; N], TrustedPeerError> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TrustedPeerError::InvalidFormat);
    }

    let bytes = value.as_bytes();
    let mut output = [0u8; N];
    for (index, slot) in output.iter_mut().enumerate() {
        let high = decode_nibble(bytes[index * 2])?;
        let low = decode_nibble(bytes[index * 2 + 1])?;
        *slot = (high << 4) | low;
    }
    Ok(output)
}

fn decode_nibble(value: u8) -> Result<u8, TrustedPeerError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(TrustedPeerError::InvalidFormat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer(seed: u8, name: &str) -> TrustedPeer {
        let signing = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        let public_key = signing.verifying_key().to_bytes();
        TrustedPeer {
            peer_name: name.to_string(),
            public_key,
            fingerprint: peer_identity::peer_fingerprint(&public_key),
        }
    }

    #[test]
    fn store_round_trips_and_sorts_by_fingerprint() {
        let mut peers = vec![test_peer(7, "Seven"), test_peer(3, "Three")];
        peers.sort_unstable_by_key(|peer| peer.fingerprint);
        let store = TrustedPeerStore { peers };
        let encoded = encode_store(&store).expect("encode");
        assert_eq!(decode_store(&encoded).expect("decode"), store);
    }

    #[test]
    fn duplicate_peer_keys_are_rejected() {
        let peer = test_peer(9, "Nine");
        let store = TrustedPeerStore {
            peers: vec![peer.clone(), peer],
        };
        assert_eq!(
            validate_store(&store),
            Err(TrustedPeerError::InvalidFormat)
        );
    }

    #[test]
    fn fingerprint_mismatch_is_rejected() {
        let mut peer = test_peer(11, "Eleven");
        peer.fingerprint[0] ^= 0x80;
        assert_eq!(
            peer.validate(),
            Err(TrustedPeerError::FingerprintMismatch)
        );
    }
}
