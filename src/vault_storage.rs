use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

const MAGIC: &[u8; 8] = b"DGMVLT01";
const FORMAT_VERSION: u16 = 1;
const CIPHER_XCHACHA20_POLY1305: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const KDF_SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const AEAD_TAG_BYTES: usize = 16;
const HEADER_BYTES: usize = 72;

const CURRENT_FILE: &str = "vault.dgmv";
const PREVIOUS_FILE: &str = "vault.previous.dgmv";
const PENDING_FILE: &str = "vault.pending.dgmv";

pub const MAX_VAULT_PLAINTEXT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_KDF_MEMORY_KIB: u32 = 1024 * 1024;
pub const MAX_KDF_ITERATIONS: u32 = 64;
pub const MAX_KDF_PARALLELISM: u32 = 64;

#[must_use]
pub struct VaultKey {
    bytes: [u8; 32],
}

impl VaultKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaultKey([REDACTED])")
    }
}

#[must_use]
pub struct VaultPlaintext {
    bytes: Vec<u8>,
}

impl VaultPlaintext {
    pub fn expose_secret(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Drop for VaultPlaintext {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for VaultPlaintext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaultPlaintext([REDACTED])")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VaultKdfMetadata {
    salt: [u8; KDF_SALT_BYTES],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

impl VaultKdfMetadata {
    pub fn argon2id(
        salt: [u8; KDF_SALT_BYTES],
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
    ) -> Result<Self, VaultError> {
        if memory_kib == 0
            || memory_kib > MAX_KDF_MEMORY_KIB
            || iterations == 0
            || iterations > MAX_KDF_ITERATIONS
            || parallelism == 0
            || parallelism > MAX_KDF_PARALLELISM
        {
            return Err(VaultError::InvalidKdfMetadata);
        }

        Ok(Self {
            salt,
            memory_kib,
            iterations,
            parallelism,
        })
    }

    pub fn salt(&self) -> &[u8; KDF_SALT_BYTES] {
        &self.salt
    }

    pub fn memory_kib(&self) -> u32 {
        self.memory_kib
    }

    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    pub fn parallelism(&self) -> u32 {
        self.parallelism
    }
}

impl fmt::Debug for VaultKdfMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultKdfMetadata")
            .field("algorithm", &"Argon2id")
            .field("salt", &"[PUBLIC KDF SALT]")
            .field("memory_kib", &self.memory_kib)
            .field("iterations", &self.iterations)
            .field("parallelism", &self.parallelism)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    VaultNotFound,
    UnsafeStorageLayout,
    InvalidFormat,
    UnsupportedFormat,
    InvalidKdfMetadata,
    PayloadTooLarge,
    RandomSourceUnavailable,
    AuthenticationFailed,
    VerificationFailed,
    RollbackFailed,
    Io,
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::VaultNotFound => "encrypted vault not found",
            Self::UnsafeStorageLayout => "vault storage layout is unsafe",
            Self::InvalidFormat => "vault format is invalid",
            Self::UnsupportedFormat => "vault format is unsupported",
            Self::InvalidKdfMetadata => "vault key-derivation metadata is invalid",
            Self::PayloadTooLarge => "vault payload exceeds the supported size limit",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::AuthenticationFailed => "vault authentication failed",
            Self::VerificationFailed => "vault durable-write verification failed",
            Self::RollbackFailed => "vault rollback failed",
            Self::Io => "vault storage I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for VaultError {}

impl From<io::Error> for VaultError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

#[derive(Debug)]
pub struct VaultStore {
    root: PathBuf,
}

impl VaultStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_kdf_metadata(&self) -> Result<VaultKdfMetadata, VaultError> {
        ensure_existing_root(&self.root)?;

        for path in [self.current_path(), self.pending_path(), self.previous_path()] {
            if fixed_file_state(&path)? == FixedFileState::Regular {
                let encoded = read_bounded_file(&path)?;
                let (header, _) = parse_encoded(&encoded)?;
                return Ok(header.kdf);
            }
        }

        Err(VaultError::VaultNotFound)
    }

    pub fn load(&self, key: &VaultKey) -> Result<VaultPlaintext, VaultError> {
        ensure_existing_root(&self.root)?;

        if !self.recover_interrupted_write(key)? {
            return Err(VaultError::VaultNotFound);
        }

        decrypt_file(&self.current_path(), key)
    }

    pub fn save(
        &self,
        key: &VaultKey,
        kdf: VaultKdfMetadata,
        plaintext: &[u8],
    ) -> Result<(), VaultError> {
        if plaintext.len() > MAX_VAULT_PLAINTEXT_BYTES {
            return Err(VaultError::PayloadTooLarge);
        }

        ensure_storage_root(&self.root)?;

        let _ = self.recover_interrupted_write(key)?;

        remove_fixed_file_if_present(&self.pending_path())?;

        let encoded = encrypt_payload(key, kdf, plaintext)?;
        write_pending(&self.pending_path(), &encoded)?;
        sync_directory(&self.root)?;

        let verification = decrypt_file(&self.pending_path(), key)?;
        if verification.expose_secret() != plaintext {
            remove_fixed_file_if_present(&self.pending_path())?;
            return Err(VaultError::VerificationFailed);
        }
        drop(verification);

        self.commit_pending(key, plaintext)
    }

    fn commit_pending(&self, key: &VaultKey, expected: &[u8]) -> Result<(), VaultError> {
        let current = self.current_path();
        let previous = self.previous_path();
        let pending = self.pending_path();

        if fixed_file_state(&pending)? != FixedFileState::Regular {
            return Err(VaultError::VerificationFailed);
        }

        remove_fixed_file_if_present(&previous)?;

        let had_current = fixed_file_state(&current)? == FixedFileState::Regular;
        if had_current {
            fs::rename(&current, &previous).map_err(|_| VaultError::Io)?;
            sync_directory(&self.root)?;
        }

        if fs::rename(&pending, &current).is_err() {
            if had_current && fs::rename(&previous, &current).is_err() {
                return Err(VaultError::RollbackFailed);
            }
            let _ = sync_directory(&self.root);
            return Err(VaultError::Io);
        }

        sync_directory(&self.root)?;

        let verified = match decrypt_file(&current, key) {
            Ok(value) => value,
            Err(_) => {
                return self.rollback_after_failed_replace(had_current);
            }
        };

        let matches = verified.expose_secret() == expected;
        drop(verified);

        if !matches {
            return self.rollback_after_failed_replace(had_current);
        }

        if had_current {
            remove_fixed_file_if_present(&previous)?;
            sync_directory(&self.root)?;
        }

        Ok(())
    }

    fn rollback_after_failed_replace(&self, had_previous: bool) -> Result<(), VaultError> {
        let current = self.current_path();
        let previous = self.previous_path();

        remove_fixed_file_if_present(&current)?;

        if had_previous {
            if fs::rename(&previous, &current).is_err() {
                return Err(VaultError::RollbackFailed);
            }
            sync_directory(&self.root)?;
        }

        Err(VaultError::VerificationFailed)
    }

    fn recover_interrupted_write(&self, key: &VaultKey) -> Result<bool, VaultError> {
        let current = self.current_path();
        let previous = self.previous_path();
        let pending = self.pending_path();

        let current_state = fixed_file_state(&current)?;
        let previous_state = fixed_file_state(&previous)?;
        let pending_state = fixed_file_state(&pending)?;

        if current_state == FixedFileState::Regular {
            match decrypt_file(&current, key) {
                Ok(value) => {
                    drop(value);
                    remove_fixed_file_if_present(&pending)?;
                    remove_fixed_file_if_present(&previous)?;
                    sync_directory(&self.root)?;
                    return Ok(true);
                }
                Err(error) if is_content_failure(error) => {
                    if previous_state == FixedFileState::Regular {
                        if let Ok(value) = decrypt_file(&previous, key) {
                            drop(value);
                            remove_fixed_file_if_present(&current)?;
                            fs::rename(&previous, &current)
                                .map_err(|_| VaultError::RollbackFailed)?;
                            remove_fixed_file_if_present(&pending)?;
                            sync_directory(&self.root)?;
                            return Ok(true);
                        }
                    }

                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }

        if pending_state == FixedFileState::Regular {
            match decrypt_file(&pending, key) {
                Ok(value) => {
                    drop(value);
                    fs::rename(&pending, &current).map_err(|_| VaultError::Io)?;
                    sync_directory(&self.root)?;
                    remove_fixed_file_if_present(&previous)?;
                    sync_directory(&self.root)?;
                    return Ok(true);
                }
                Err(error) if is_content_failure(error) => {
                    remove_fixed_file_if_present(&pending)?;
                }
                Err(error) => return Err(error),
            }
        }

        if previous_state == FixedFileState::Regular {
            let value = decrypt_file(&previous, key)?;
            drop(value);
            fs::rename(&previous, &current).map_err(|_| VaultError::RollbackFailed)?;
            sync_directory(&self.root)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn current_path(&self) -> PathBuf {
        self.root.join(CURRENT_FILE)
    }

    fn previous_path(&self) -> PathBuf {
        self.root.join(PREVIOUS_FILE)
    }

    fn pending_path(&self) -> PathBuf {
        self.root.join(PENDING_FILE)
    }
}

fn is_content_failure(error: VaultError) -> bool {
    matches!(
        error,
        VaultError::AuthenticationFailed
            | VaultError::InvalidFormat
            | VaultError::UnsupportedFormat
            | VaultError::InvalidKdfMetadata
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixedFileState {
    Missing,
    Regular,
}

fn fixed_file_state(path: &Path) -> Result<FixedFileState, VaultError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(VaultError::UnsafeStorageLayout);
            }
            Ok(FixedFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FixedFileState::Missing),
        Err(_) => Err(VaultError::Io),
    }
}

fn ensure_existing_root(root: &Path) -> Result<(), VaultError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(VaultError::UnsafeStorageLayout);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(VaultError::VaultNotFound),
        Err(_) => Err(VaultError::Io),
    }
}

fn ensure_storage_root(root: &Path) -> Result<(), VaultError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(VaultError::UnsafeStorageLayout);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|_| VaultError::Io)?;
            let metadata = fs::symlink_metadata(root).map_err(|_| VaultError::Io)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(VaultError::UnsafeStorageLayout);
            }
        }
        Err(_) => return Err(VaultError::Io),
    }

    Ok(())
}

fn remove_fixed_file_if_present(path: &Path) -> Result<(), VaultError> {
    match fixed_file_state(path)? {
        FixedFileState::Missing => Ok(()),
        FixedFileState::Regular => fs::remove_file(path).map_err(|_| VaultError::Io),
    }
}

fn write_pending(path: &Path, encoded: &[u8]) -> Result<(), VaultError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| VaultError::Io)?;
    file.write_all(encoded).map_err(|_| VaultError::Io)?;
    file.sync_all().map_err(|_| VaultError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), VaultError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| VaultError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), VaultError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    let result = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .and_then(|directory| directory.sync_all());

    // Windows does not provide a fully portable directory-fsync contract
    // through std. File contents are always sync_all'd; directory metadata
    // flushing is attempted when the filesystem accepts it.
    let _ = result;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> Result<(), VaultError> {
    Ok(())
}

fn read_bounded_file(path: &Path) -> Result<Vec<u8>, VaultError> {
    if fixed_file_state(path)? != FixedFileState::Regular {
        return Err(VaultError::VaultNotFound);
    }

    let metadata = fs::metadata(path).map_err(|_| VaultError::Io)?;
    let max_file_bytes = HEADER_BYTES as u64
        + MAX_VAULT_PLAINTEXT_BYTES as u64
        + AEAD_TAG_BYTES as u64;

    if metadata.len() < HEADER_BYTES as u64 || metadata.len() > max_file_bytes {
        return Err(VaultError::InvalidFormat);
    }

    fs::read(path).map_err(|_| VaultError::Io)
}

fn encrypt_payload(
    key: &VaultKey,
    kdf: VaultKdfMetadata,
    plaintext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    if plaintext.len() > MAX_VAULT_PLAINTEXT_BYTES {
        return Err(VaultError::PayloadTooLarge);
    }

    let ciphertext_len = plaintext
        .len()
        .checked_add(AEAD_TAG_BYTES)
        .ok_or(VaultError::PayloadTooLarge)?;

    let mut nonce = [0_u8; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| VaultError::RandomSourceUnavailable)?;

    let header = VaultHeader {
        kdf,
        nonce,
        ciphertext_len: ciphertext_len as u64,
    };
    let header_bytes = header.encode();

    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| VaultError::AuthenticationFailed)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&header.nonce),
            Payload {
                msg: plaintext,
                aad: &header_bytes,
            },
        )
        .map_err(|_| VaultError::AuthenticationFailed)?;

    if ciphertext.len() != ciphertext_len {
        return Err(VaultError::VerificationFailed);
    }

    let mut encoded = Vec::with_capacity(HEADER_BYTES + ciphertext.len());
    encoded.extend_from_slice(&header_bytes);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

fn decrypt_file(path: &Path, key: &VaultKey) -> Result<VaultPlaintext, VaultError> {
    let encoded = read_bounded_file(path)?;
    decrypt_encoded(&encoded, key)
}

fn decrypt_encoded(encoded: &[u8], key: &VaultKey) -> Result<VaultPlaintext, VaultError> {
    let (header, ciphertext) = parse_encoded(encoded)?;
    let header_bytes = header.encode();

    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| VaultError::AuthenticationFailed)?;
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&header.nonce),
            Payload {
                msg: ciphertext,
                aad: &header_bytes,
            },
        )
        .map_err(|_| VaultError::AuthenticationFailed)?;

    if plaintext.len() > MAX_VAULT_PLAINTEXT_BYTES {
        return Err(VaultError::PayloadTooLarge);
    }

    Ok(VaultPlaintext { bytes: plaintext })
}

#[derive(Clone, Copy)]
struct VaultHeader {
    kdf: VaultKdfMetadata,
    nonce: [u8; NONCE_BYTES],
    ciphertext_len: u64,
}

impl VaultHeader {
    fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut output = [0_u8; HEADER_BYTES];

        output[0..8].copy_from_slice(MAGIC);
        output[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        output[10] = CIPHER_XCHACHA20_POLY1305;
        output[11] = KDF_ARGON2ID;
        output[12..28].copy_from_slice(&self.kdf.salt);
        output[28..32].copy_from_slice(&self.kdf.memory_kib.to_le_bytes());
        output[32..36].copy_from_slice(&self.kdf.iterations.to_le_bytes());
        output[36..40].copy_from_slice(&self.kdf.parallelism.to_le_bytes());
        output[40..64].copy_from_slice(&self.nonce);
        output[64..72].copy_from_slice(&self.ciphertext_len.to_le_bytes());

        output
    }

    fn parse(input: &[u8]) -> Result<Self, VaultError> {
        if input.len() != HEADER_BYTES {
            return Err(VaultError::InvalidFormat);
        }

        if &input[0..8] != MAGIC {
            return Err(VaultError::InvalidFormat);
        }

        let version = u16::from_le_bytes([input[8], input[9]]);
        if version != FORMAT_VERSION {
            return Err(VaultError::UnsupportedFormat);
        }

        if input[10] != CIPHER_XCHACHA20_POLY1305 || input[11] != KDF_ARGON2ID {
            return Err(VaultError::UnsupportedFormat);
        }

        let mut salt = [0_u8; KDF_SALT_BYTES];
        salt.copy_from_slice(&input[12..28]);

        let memory_kib = u32::from_le_bytes([input[28], input[29], input[30], input[31]]);
        let iterations = u32::from_le_bytes([input[32], input[33], input[34], input[35]]);
        let parallelism = u32::from_le_bytes([input[36], input[37], input[38], input[39]]);
        let kdf = VaultKdfMetadata::argon2id(salt, memory_kib, iterations, parallelism)?;

        let mut nonce = [0_u8; NONCE_BYTES];
        nonce.copy_from_slice(&input[40..64]);

        let ciphertext_len = u64::from_le_bytes([
            input[64], input[65], input[66], input[67], input[68], input[69], input[70], input[71],
        ]);

        let max_ciphertext = MAX_VAULT_PLAINTEXT_BYTES as u64 + AEAD_TAG_BYTES as u64;
        if ciphertext_len < AEAD_TAG_BYTES as u64 || ciphertext_len > max_ciphertext {
            return Err(VaultError::InvalidFormat);
        }

        Ok(Self {
            kdf,
            nonce,
            ciphertext_len,
        })
    }
}

fn parse_encoded(encoded: &[u8]) -> Result<(VaultHeader, &[u8]), VaultError> {
    if encoded.len() < HEADER_BYTES {
        return Err(VaultError::InvalidFormat);
    }

    let header = VaultHeader::parse(&encoded[..HEADER_BYTES])?;
    let ciphertext_len =
        usize::try_from(header.ciphertext_len).map_err(|_| VaultError::InvalidFormat)?;

    if encoded.len() != HEADER_BYTES + ciphertext_len {
        return Err(VaultError::InvalidFormat);
    }

    Ok((header, &encoded[HEADER_BYTES..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const PAYLOAD: &[u8] = b"SYNTHETIC_VAULT_PAYLOAD";

    fn temporary_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-vault-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn key(byte: u8) -> VaultKey {
        VaultKey::from_bytes([byte; 32])
    }

    fn metadata() -> VaultKdfMetadata {
        VaultKdfMetadata::argon2id([7_u8; 16], 65_536, 3, 1).expect("metadata")
    }

    #[test]
    fn round_trip_uses_authenticated_encryption() {
        let root = temporary_root("roundtrip");
        let store = VaultStore::new(&root);
        let key = key(0x42);

        store.save(&key, metadata(), PAYLOAD).expect("save");
        let loaded = store.load(&key).expect("load");

        assert_eq!(loaded.expose_secret(), PAYLOAD);

        let encoded = fs::read(root.join(CURRENT_FILE)).expect("encoded vault");
        assert!(!encoded.windows(PAYLOAD.len()).any(|window| window == PAYLOAD));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn wrong_key_is_rejected_without_plaintext() {
        let root = temporary_root("wrong-key");
        let store = VaultStore::new(&root);
        let key_a = key(0x11);
        let key_b = key(0x22);

        store.save(&key_a, metadata(), PAYLOAD).expect("save");

        assert_eq!(store.load(&key_b).unwrap_err(), VaultError::AuthenticationFailed);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn ciphertext_tampering_is_rejected() {
        let root = temporary_root("tamper");
        let store = VaultStore::new(&root);
        let key = key(0x31);

        store.save(&key, metadata(), PAYLOAD).expect("save");

        let path = root.join(CURRENT_FILE);
        let mut encoded = fs::read(&path).expect("read");
        let last = encoded.len() - 1;
        encoded[last] ^= 0x01;
        fs::write(&path, encoded).expect("tamper");

        assert_eq!(store.load(&key).unwrap_err(), VaultError::AuthenticationFailed);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn authenticated_header_rejects_metadata_tampering() {
        let root = temporary_root("header-tamper");
        let store = VaultStore::new(&root);
        let key = key(0x51);

        store.save(&key, metadata(), PAYLOAD).expect("save");

        let path = root.join(CURRENT_FILE);
        let mut encoded = fs::read(&path).expect("read");
        encoded[32] ^= 0x01;
        fs::write(&path, encoded).expect("tamper");

        assert_eq!(store.load(&key).unwrap_err(), VaultError::AuthenticationFailed);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn kdf_metadata_is_available_before_unlock() {
        let root = temporary_root("metadata");
        let store = VaultStore::new(&root);
        let key = key(0x61);
        let expected = metadata();

        store.save(&key, expected, PAYLOAD).expect("save");

        assert_eq!(store.read_kdf_metadata().expect("metadata"), expected);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn interrupted_replace_with_previous_rolls_back() {
        let root = temporary_root("rollback");
        let store = VaultStore::new(&root);
        let key = key(0x71);

        store.save(&key, metadata(), PAYLOAD).expect("save");

        let current = root.join(CURRENT_FILE);
        let previous = root.join(PREVIOUS_FILE);
        fs::copy(&current, &previous).expect("previous");

        let mut damaged = fs::read(&current).expect("current");
        let last = damaged.len() - 1;
        damaged[last] ^= 0x40;
        fs::write(&current, damaged).expect("damage current");

        let loaded = store.load(&key).expect("rollback load");
        assert_eq!(loaded.expose_secret(), PAYLOAD);
        assert!(current.is_file());
        assert!(!previous.exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn fully_written_pending_is_recovered_when_current_is_missing() {
        let root = temporary_root("pending-recovery");
        let store = VaultStore::new(&root);
        let key = key(0x81);

        ensure_storage_root(&root).expect("root");
        let encoded = encrypt_payload(&key, metadata(), PAYLOAD).expect("encrypt");
        write_pending(&root.join(PENDING_FILE), &encoded).expect("pending");

        let loaded = store.load(&key).expect("recover pending");
        assert_eq!(loaded.expose_secret(), PAYLOAD);
        assert!(root.join(CURRENT_FILE).is_file());
        assert!(!root.join(PENDING_FILE).exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn successful_replacement_cleans_previous_and_pending() {
        let root = temporary_root("replace");
        let store = VaultStore::new(&root);
        let key = key(0x91);

        store.save(&key, metadata(), PAYLOAD).expect("first");
        store
            .save(&key, metadata(), b"SYNTHETIC_VAULT_PAYLOAD_SECOND")
            .expect("second");

        assert!(root.join(CURRENT_FILE).is_file());
        assert!(!root.join(PREVIOUS_FILE).exists());
        assert!(!root.join(PENDING_FILE).exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn secret_debug_output_is_redacted() {
        let key = key(0xA1);
        let plaintext = VaultPlaintext {
            bytes: PAYLOAD.to_vec(),
        };

        assert_eq!(format!("{key:?}"), "VaultKey([REDACTED])");
        assert_eq!(format!("{plaintext:?}"), "VaultPlaintext([REDACTED])");
    }

    #[test]
    fn rejects_oversized_payload_before_writing() {
        let root = temporary_root("oversized");
        let store = VaultStore::new(&root);
        let key = key(0xB1);
        let payload = vec![0_u8; MAX_VAULT_PLAINTEXT_BYTES + 1];

        assert_eq!(
            store.save(&key, metadata(), &payload).unwrap_err(),
            VaultError::PayloadTooLarge
        );
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_storage_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = temporary_root("symlink-root");
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, &root).expect("symlink");

        let store = VaultStore::new(&root);
        let key = key(0xC1);

        assert_eq!(
            store.save(&key, metadata(), PAYLOAD).unwrap_err(),
            VaultError::UnsafeStorageLayout
        );

        fs::remove_file(&root).expect("remove link");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
