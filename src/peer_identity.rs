use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const IDENTITY_MAGIC: &[u8; 8] = b"DGMPRID1";
const IDENTITY_VERSION: u16 = 1;
const IDENTITY_FILE: &str = "peer_identity.v1";
const IDENTITY_PENDING_FILE: &str = "peer_identity.v1.pending";
const IDENTITY_FILE_BYTES: usize = 8 + 2 + PEER_PUBLIC_KEY_BYTES * 3;

const FINGERPRINT_DOMAIN: &[u8] = b"DANGOMUSHI-PEER-IDENTITY-FINGERPRINT-V1\0";
const VERIFICATION_DOMAIN: &[u8] = b"DANGOMUSHI-PEER-IDENTITY-VERIFY-V1\0";

pub const PEER_PUBLIC_KEY_BYTES: usize = 32;
pub const PEER_FINGERPRINT_BYTES: usize = 32;
pub const PEER_SIGNATURE_BYTES: usize = 64;
pub const PEER_VERIFICATION_CHALLENGE_BYTES: usize = 32;
pub const MAX_PEER_NAME_BYTES: usize = 128;
pub const PEER_VERIFICATION_LIFETIME: Duration = Duration::from_secs(5 * 60);

pub trait PeerRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PeerRandomSourceError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerRandomSourceError;

impl fmt::Display for PeerRandomSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("peer random source unavailable")
    }
}

impl Error for PeerRandomSourceError {}

#[derive(Debug, Default)]
pub struct OsPeerRandom;

impl PeerRandomSource for OsPeerRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PeerRandomSourceError> {
        getrandom::fill(destination).map_err(|_| PeerRandomSourceError)
    }
}

pub struct LocalPeerIdentity {
    secret_seed: Zeroizing<[u8; PEER_PUBLIC_KEY_BYTES]>,
    public_key: [u8; PEER_PUBLIC_KEY_BYTES],
    fingerprint: [u8; PEER_FINGERPRINT_BYTES],
}

impl LocalPeerIdentity {
    pub const fn public_key(&self) -> &[u8; PEER_PUBLIC_KEY_BYTES] {
        &self.public_key
    }

    pub const fn fingerprint(&self) -> &[u8; PEER_FINGERPRINT_BYTES] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }

    pub const fn summary(&self) -> PeerIdentitySummary {
        PeerIdentitySummary {
            public_key: self.public_key,
            fingerprint: self.fingerprint,
        }
    }

    pub fn sign_verification(
        &self,
        challenge: &[u8; PEER_VERIFICATION_CHALLENGE_BYTES],
        peer_name: &str,
    ) -> Result<[u8; PEER_SIGNATURE_BYTES], PeerIdentityError> {
        let message = peer_verification_message(challenge, peer_name, &self.public_key)?;
        let signing_key = SigningKey::from_bytes(&*self.secret_seed);
        Ok(signing_key.sign(&message).to_bytes())
    }

    pub(crate) fn sign_message(&self, message: &[u8]) -> [u8; PEER_SIGNATURE_BYTES] {
        let signing_key = SigningKey::from_bytes(&*self.secret_seed);
        signing_key.sign(message).to_bytes()
    }
}

impl fmt::Debug for LocalPeerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalPeerIdentity")
            .field("secret_seed", &"[REDACTED]")
            .field("fingerprint", &self.fingerprint_hex())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerIdentitySummary {
    public_key: [u8; PEER_PUBLIC_KEY_BYTES],
    fingerprint: [u8; PEER_FINGERPRINT_BYTES],
}

impl PeerIdentitySummary {
    pub const fn public_key(&self) -> &[u8; PEER_PUBLIC_KEY_BYTES] {
        &self.public_key
    }

    pub const fn fingerprint(&self) -> &[u8; PEER_FINGERPRINT_BYTES] {
        &self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        hex_upper(&self.fingerprint)
    }
}

pub struct PeerVerificationChallenge {
    bytes: [u8; PEER_VERIFICATION_CHALLENGE_BYTES],
    expires_at: Instant,
}

impl PeerVerificationChallenge {
    pub const fn bytes(&self) -> &[u8; PEER_VERIFICATION_CHALLENGE_BYTES] {
        &self.bytes
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerVerificationSubmission {
    pub peer_name: String,
    pub public_key: [u8; PEER_PUBLIC_KEY_BYTES],
    pub signature: [u8; PEER_SIGNATURE_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPeerCandidate {
    peer_name: String,
    public_key: [u8; PEER_PUBLIC_KEY_BYTES],
    fingerprint: [u8; PEER_FINGERPRINT_BYTES],
}

impl VerifiedPeerCandidate {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerIdentityError {
    WorkspaceUnavailable,
    UnsafeStateLayout,
    InvalidFormat,
    AlreadyInitialized,
    InvalidPeerName,
    RandomSourceUnavailable,
    VerificationExpired,
    ProofRejected,
    VerificationFailed,
    Io,
}

impl fmt::Display for PeerIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace peer-identity storage is unavailable",
            Self::UnsafeStateLayout => "peer-identity state layout is unsafe",
            Self::InvalidFormat => "peer-identity format is invalid",
            Self::AlreadyInitialized => "local peer identity is already initialized",
            Self::InvalidPeerName => "peer display name is invalid",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::VerificationExpired => "peer identity verification challenge expired",
            Self::ProofRejected => "peer identity proof was rejected",
            Self::VerificationFailed => "peer-identity storage verification failed",
            Self::Io => "peer-identity storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for PeerIdentityError {}

pub fn load(workspace_root: &Path) -> Result<Option<LocalPeerIdentity>, PeerIdentityError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_identity(&state_root)?;
    load_committed(&state_root)
}

pub fn initialize(workspace_root: &Path) -> Result<LocalPeerIdentity, PeerIdentityError> {
    let mut random = OsPeerRandom;
    initialize_with_source(workspace_root, &mut random)
}

pub fn initialize_with_source(
    workspace_root: &Path,
    random: &mut impl PeerRandomSource,
) -> Result<LocalPeerIdentity, PeerIdentityError> {
    let state_root = checked_state_root(workspace_root)?;
    recover_interrupted_identity(&state_root)?;

    let target = state_root.join(IDENTITY_FILE);
    if fixed_file_state(&target)? == FixedFileState::Regular {
        return Err(PeerIdentityError::AlreadyInitialized);
    }

    let pending = state_root.join(IDENTITY_PENDING_FILE);
    if fixed_file_state(&pending)? != FixedFileState::Missing {
        return Err(PeerIdentityError::UnsafeStateLayout);
    }

    let mut seed = [0u8; PEER_PUBLIC_KEY_BYTES];
    random
        .fill_bytes(&mut seed)
        .map_err(|_| PeerIdentityError::RandomSourceUnavailable)?;
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let identity = LocalPeerIdentity {
        secret_seed: Zeroizing::new(seed),
        public_key,
        fingerprint: peer_fingerprint(&public_key),
    };

    let encoded = encode_identity(&identity);
    write_create_new(&pending, &encoded)?;

    let pending_identity = read_identity_file(&pending)?;
    if !same_identity(&identity, &pending_identity) {
        let _ = fs::remove_file(&pending);
        return Err(PeerIdentityError::VerificationFailed);
    }

    if fs::rename(&pending, &target).is_err() {
        let _ = fs::remove_file(&pending);
        return Err(PeerIdentityError::Io);
    }
    sync_directory(&state_root)?;

    let committed = read_identity_file(&target)?;
    if !same_identity(&identity, &committed) {
        return Err(PeerIdentityError::VerificationFailed);
    }

    Ok(committed)
}

pub fn begin_verification(
    now: Instant,
) -> Result<PeerVerificationChallenge, PeerIdentityError> {
    let mut random = OsPeerRandom;
    begin_verification_with_source(now, &mut random)
}

pub fn begin_verification_with_source(
    now: Instant,
    random: &mut impl PeerRandomSource,
) -> Result<PeerVerificationChallenge, PeerIdentityError> {
    let mut bytes = [0u8; PEER_VERIFICATION_CHALLENGE_BYTES];
    random
        .fill_bytes(&mut bytes)
        .map_err(|_| PeerIdentityError::RandomSourceUnavailable)?;
    Ok(PeerVerificationChallenge {
        bytes,
        expires_at: now + PEER_VERIFICATION_LIFETIME,
    })
}

pub fn complete_verification(
    now: Instant,
    challenge: PeerVerificationChallenge,
    submission: PeerVerificationSubmission,
) -> Result<VerifiedPeerCandidate, PeerIdentityError> {
    if now >= challenge.expires_at {
        return Err(PeerIdentityError::VerificationExpired);
    }

    let message =
        peer_verification_message(&challenge.bytes, &submission.peer_name, &submission.public_key)?;
    let verifying_key =
        VerifyingKey::from_bytes(&submission.public_key).map_err(|_| PeerIdentityError::ProofRejected)?;
    let signature = Signature::from_bytes(&submission.signature);
    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| PeerIdentityError::ProofRejected)?;

    Ok(VerifiedPeerCandidate {
        peer_name: submission.peer_name,
        public_key: submission.public_key,
        fingerprint: peer_fingerprint(&submission.public_key),
    })
}

pub fn peer_verification_message(
    challenge: &[u8; PEER_VERIFICATION_CHALLENGE_BYTES],
    peer_name: &str,
    public_key: &[u8; PEER_PUBLIC_KEY_BYTES],
) -> Result<Vec<u8>, PeerIdentityError> {
    validate_peer_name(peer_name)?;
    let name = peer_name.as_bytes();
    let name_len = u16::try_from(name.len()).expect("peer-name bound fits u16");

    let mut message = Vec::with_capacity(
        VERIFICATION_DOMAIN.len()
            + PEER_VERIFICATION_CHALLENGE_BYTES
            + PEER_PUBLIC_KEY_BYTES
            + 2
            + name.len(),
    );
    message.extend_from_slice(VERIFICATION_DOMAIN);
    message.extend_from_slice(challenge);
    message.extend_from_slice(public_key);
    message.extend_from_slice(&name_len.to_be_bytes());
    message.extend_from_slice(name);
    Ok(message)
}

pub(crate) fn validate_peer_name(peer_name: &str) -> Result<(), PeerIdentityError> {
    if peer_name.is_empty()
        || peer_name.len() > MAX_PEER_NAME_BYTES
        || peer_name.trim() != peer_name
        || peer_name.chars().any(char::is_control)
    {
        return Err(PeerIdentityError::InvalidPeerName);
    }
    Ok(())
}

pub(crate) fn peer_fingerprint(
    public_key: &[u8; PEER_PUBLIC_KEY_BYTES],
) -> [u8; PEER_FINGERPRINT_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_DOMAIN);
    hasher.update(public_key);
    let digest = hasher.finalize();
    let mut fingerprint = [0u8; PEER_FINGERPRINT_BYTES];
    fingerprint.copy_from_slice(&digest);
    fingerprint
}

fn checked_state_root(workspace_root: &Path) -> Result<PathBuf, PeerIdentityError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| PeerIdentityError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(PeerIdentityError::WorkspaceUnavailable);
    }

    let marker = canonical_workspace.join(".dangomushi-workspace");
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| PeerIdentityError::WorkspaceUnavailable)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(PeerIdentityError::UnsafeStateLayout);
    }

    let state = canonical_workspace.join("state");
    let metadata =
        fs::symlink_metadata(&state).map_err(|_| PeerIdentityError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PeerIdentityError::UnsafeStateLayout);
    }

    let canonical_state =
        fs::canonicalize(&state).map_err(|_| PeerIdentityError::WorkspaceUnavailable)?;
    if !canonical_state.starts_with(&canonical_workspace) {
        return Err(PeerIdentityError::UnsafeStateLayout);
    }

    Ok(canonical_state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, PeerIdentityError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(PeerIdentityError::UnsafeStateLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(PeerIdentityError::Io),
    }
}

fn recover_interrupted_identity(state_root: &Path) -> Result<(), PeerIdentityError> {
    let target = state_root.join(IDENTITY_FILE);
    let pending = state_root.join(IDENTITY_PENDING_FILE);

    let target_state = fixed_file_state(&target)?;
    let pending_state = fixed_file_state(&pending)?;

    match (target_state, pending_state) {
        (FixedFileState::Missing, FixedFileState::Missing) => Ok(()),
        (FixedFileState::Regular, FixedFileState::Missing) => {
            read_identity_file(&target)?;
            Ok(())
        }
        (FixedFileState::Missing, FixedFileState::Regular) => {
            let pending_identity = read_identity_file(&pending)?;
            fs::rename(&pending, &target).map_err(|_| PeerIdentityError::Io)?;
            sync_directory(state_root)?;
            let committed = read_identity_file(&target)?;
            if same_identity(&pending_identity, &committed) {
                Ok(())
            } else {
                Err(PeerIdentityError::VerificationFailed)
            }
        }
        (FixedFileState::Regular, FixedFileState::Regular) => {
            Err(PeerIdentityError::UnsafeStateLayout)
        }
    }
}

fn load_committed(state_root: &Path) -> Result<Option<LocalPeerIdentity>, PeerIdentityError> {
    let target = state_root.join(IDENTITY_FILE);
    match fixed_file_state(&target)? {
        FixedFileState::Missing => Ok(None),
        FixedFileState::Regular => read_identity_file(&target).map(Some),
    }
}

fn encode_identity(identity: &LocalPeerIdentity) -> Zeroizing<Vec<u8>> {
    let mut encoded = Vec::with_capacity(IDENTITY_FILE_BYTES);
    encoded.extend_from_slice(IDENTITY_MAGIC);
    encoded.extend_from_slice(&IDENTITY_VERSION.to_be_bytes());
    encoded.extend_from_slice(&*identity.secret_seed);
    encoded.extend_from_slice(&identity.public_key);
    encoded.extend_from_slice(&identity.fingerprint);
    debug_assert_eq!(encoded.len(), IDENTITY_FILE_BYTES);
    Zeroizing::new(encoded)
}

fn read_identity_file(path: &Path) -> Result<LocalPeerIdentity, PeerIdentityError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(PeerIdentityError::InvalidFormat);
    }
    let metadata = fs::metadata(path).map_err(|_| PeerIdentityError::Io)?;
    if metadata.len() != IDENTITY_FILE_BYTES as u64 {
        return Err(PeerIdentityError::InvalidFormat);
    }
    let bytes = Zeroizing::new(fs::read(path).map_err(|_| PeerIdentityError::Io)?);
    decode_identity(&bytes)
}

fn decode_identity(bytes: &[u8]) -> Result<LocalPeerIdentity, PeerIdentityError> {
    if bytes.len() != IDENTITY_FILE_BYTES || &bytes[..8] != IDENTITY_MAGIC {
        return Err(PeerIdentityError::InvalidFormat);
    }

    let version = u16::from_be_bytes([bytes[8], bytes[9]]);
    if version != IDENTITY_VERSION {
        return Err(PeerIdentityError::InvalidFormat);
    }

    let mut seed = [0u8; PEER_PUBLIC_KEY_BYTES];
    seed.copy_from_slice(&bytes[10..42]);
    let mut public_key = [0u8; PEER_PUBLIC_KEY_BYTES];
    public_key.copy_from_slice(&bytes[42..74]);
    let mut fingerprint = [0u8; PEER_FINGERPRINT_BYTES];
    fingerprint.copy_from_slice(&bytes[74..106]);

    let signing_key = SigningKey::from_bytes(&seed);
    if signing_key.verifying_key().to_bytes() != public_key
        || peer_fingerprint(&public_key) != fingerprint
    {
        return Err(PeerIdentityError::InvalidFormat);
    }

    Ok(LocalPeerIdentity {
        secret_seed: Zeroizing::new(seed),
        public_key,
        fingerprint,
    })
}

fn same_identity(left: &LocalPeerIdentity, right: &LocalPeerIdentity) -> bool {
    *left.secret_seed == *right.secret_seed
        && left.public_key == right.public_key
        && left.fingerprint == right.fingerprint
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), PeerIdentityError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| PeerIdentityError::Io)?;
    file.write_all(bytes).map_err(|_| PeerIdentityError::Io)?;
    file.sync_all().map_err(|_| PeerIdentityError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PeerIdentityError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PeerIdentityError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), PeerIdentityError> {
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
fn sync_directory(_path: &Path) -> Result<(), PeerIdentityError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedRandom {
        bytes: [u8; PEER_VERIFICATION_CHALLENGE_BYTES],
    }

    impl PeerRandomSource for FixedRandom {
        fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), PeerRandomSourceError> {
            destination.copy_from_slice(&self.bytes[..destination.len()]);
            Ok(())
        }
    }

    #[test]
    fn peer_fingerprint_is_domain_separated_and_deterministic() {
        let key = [7u8; PEER_PUBLIC_KEY_BYTES];
        assert_eq!(peer_fingerprint(&key), peer_fingerprint(&key));
        let bare: [u8; 32] = Sha256::digest(key).into();
        assert_ne!(peer_fingerprint(&key), bare);
    }

    #[test]
    fn verification_challenge_is_bounded_and_expires() {
        let now = Instant::now();
        let mut random = FixedRandom { bytes: [9u8; 32] };
        let challenge =
            begin_verification_with_source(now, &mut random).expect("challenge");
        assert_eq!(challenge.bytes(), &[9u8; 32]);
        assert_eq!(challenge.remaining(now), PEER_VERIFICATION_LIFETIME);
    }

    #[test]
    fn invalid_peer_names_are_rejected() {
        assert_eq!(
            validate_peer_name(" bad"),
            Err(PeerIdentityError::InvalidPeerName)
        );
        assert_eq!(
            validate_peer_name("bad\nname"),
            Err(PeerIdentityError::InvalidPeerName)
        );
    }
}
