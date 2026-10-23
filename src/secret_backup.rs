use crate::vault_storage::{
    kdf_metadata_from_encoded, VaultError, VaultStore, MAX_VAULT_ENCODED_BYTES,
};
use crate::vault_unlock::{
    derive_vault_key, MasterPassphrase, UnlockedVault, VaultAccessError,
};
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const BACKUP_MAGIC: &[u8; 8] = b"DGMBAK01";
const BACKUP_VERSION: u16 = 1;
const BACKUP_HEADER_BYTES: usize = 18;
pub const MAX_SECRET_BACKUP_BYTES: usize = BACKUP_HEADER_BYTES + MAX_VAULT_ENCODED_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackupError {
    SourceVaultUnavailable,
    BackupAlreadyExists,
    BackupNotFound,
    PendingBackupConflict,
    UnsafeBackupLayout,
    InvalidBackupFormat,
    UnsupportedBackupVersion,
    BackupAuthenticationFailed,
    RestoreTargetUnsafe,
    RestoreFailed,
    Io,
}

impl fmt::Display for SecretBackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SourceVaultUnavailable => "source encrypted vault is unavailable",
            Self::BackupAlreadyExists => "secret backup destination already exists",
            Self::BackupNotFound => "secret backup not found",
            Self::PendingBackupConflict => "secret backup pending file conflicts with this backup",
            Self::UnsafeBackupLayout => "secret backup filesystem layout is unsafe",
            Self::InvalidBackupFormat => "secret backup format is invalid",
            Self::UnsupportedBackupVersion => "secret backup format version is unsupported",
            Self::BackupAuthenticationFailed => "secret backup authentication failed",
            Self::RestoreTargetUnsafe => "secret backup restore target is not in a safe state",
            Self::RestoreFailed => "secret backup restore failed",
            Self::Io => "secret backup I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for SecretBackupError {}

pub fn create_backup(
    vault: &UnlockedVault,
    destination: impl AsRef<Path>,
) -> Result<(), SecretBackupError> {
    let ciphertext = vault
        .export_verified_ciphertext()
        .map_err(map_source_vault_error)?;
    let backup = encode_backup(&ciphertext)?;
    durable_create_new(destination.as_ref(), &backup)
}

pub fn restore_backup(
    backup_path: impl AsRef<Path>,
    destination: VaultStore,
    passphrase: MasterPassphrase,
) -> Result<UnlockedVault, SecretBackupError> {
    let backup_bytes = read_backup_file(backup_path.as_ref())?;
    let ciphertext = decode_backup(&backup_bytes)?;

    let kdf = kdf_metadata_from_encoded(ciphertext).map_err(map_backup_vault_format_error)?;
    let key = derive_vault_key(&passphrase, kdf).map_err(map_key_derivation_error)?;
    drop(passphrase);

    destination
        .restore_verified_ciphertext(&key, ciphertext)
        .map_err(map_restore_error)?;

    Ok(UnlockedVault::from_verified_parts(destination, key, kdf))
}

fn encode_backup(ciphertext: &[u8]) -> Result<Vec<u8>, SecretBackupError> {
    if ciphertext.is_empty() || ciphertext.len() > MAX_VAULT_ENCODED_BYTES {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    let length = u64::try_from(ciphertext.len())
        .map_err(|_| SecretBackupError::InvalidBackupFormat)?;

    let mut output = Vec::with_capacity(BACKUP_HEADER_BYTES + ciphertext.len());
    output.extend_from_slice(BACKUP_MAGIC);
    output.extend_from_slice(&BACKUP_VERSION.to_le_bytes());
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(ciphertext);
    Ok(output)
}

fn decode_backup(bytes: &[u8]) -> Result<&[u8], SecretBackupError> {
    if bytes.len() < BACKUP_HEADER_BYTES || bytes.len() > MAX_SECRET_BACKUP_BYTES {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    if &bytes[0..8] != BACKUP_MAGIC {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != BACKUP_VERSION {
        return Err(SecretBackupError::UnsupportedBackupVersion);
    }

    let encoded_length = u64::from_le_bytes([
        bytes[10], bytes[11], bytes[12], bytes[13],
        bytes[14], bytes[15], bytes[16], bytes[17],
    ]);
    let encoded_length =
        usize::try_from(encoded_length).map_err(|_| SecretBackupError::InvalidBackupFormat)?;

    if encoded_length == 0 || encoded_length > MAX_VAULT_ENCODED_BYTES {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    let expected_total = BACKUP_HEADER_BYTES
        .checked_add(encoded_length)
        .ok_or(SecretBackupError::InvalidBackupFormat)?;

    if expected_total != bytes.len() {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    Ok(&bytes[BACKUP_HEADER_BYTES..])
}

fn durable_create_new(path: &Path, bytes: &[u8]) -> Result<(), SecretBackupError> {
    let parent = parent_directory(path);
    ensure_backup_parent(parent)?;

    match backup_file_state(path)? {
        BackupFileState::Missing => {}
        BackupFileState::Regular => return Err(SecretBackupError::BackupAlreadyExists),
    }

    let pending = pending_path(path);
    match backup_file_state(&pending)? {
        BackupFileState::Missing => {}
        BackupFileState::Regular => {
            let existing = read_backup_file(&pending)?;
            if existing != bytes {
                return Err(SecretBackupError::PendingBackupConflict);
            }
        }
    }

    if backup_file_state(&pending)? == BackupFileState::Missing {
        write_new_file(&pending, bytes)?;
    }

    let staged = read_backup_file(&pending)?;
    if staged != bytes {
        return Err(SecretBackupError::PendingBackupConflict);
    }

    write_new_file(path, bytes)?;

    let final_bytes = read_backup_file(path)?;
    if final_bytes != bytes {
        return Err(SecretBackupError::Io);
    }

    fs::remove_file(&pending).map_err(|_| SecretBackupError::Io)?;
    sync_directory(parent)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackupFileState {
    Missing,
    Regular,
}

fn backup_file_state(path: &Path) -> Result<BackupFileState, SecretBackupError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(SecretBackupError::UnsafeBackupLayout);
            }
            Ok(BackupFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BackupFileState::Missing),
        Err(_) => Err(SecretBackupError::Io),
    }
}

fn ensure_backup_parent(parent: &Path) -> Result<(), SecretBackupError> {
    match fs::symlink_metadata(parent) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SecretBackupError::UnsafeBackupLayout);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(SecretBackupError::UnsafeBackupLayout)
        }
        Err(_) => Err(SecretBackupError::Io),
    }
}

fn pending_path(path: &Path) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(".pending");
    PathBuf::from(value)
}

fn parent_directory(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), SecretBackupError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| SecretBackupError::Io)?;
    file.write_all(bytes).map_err(|_| SecretBackupError::Io)?;
    file.sync_all().map_err(|_| SecretBackupError::Io)?;
    Ok(())
}

fn read_backup_file(path: &Path) -> Result<Vec<u8>, SecretBackupError> {
    match backup_file_state(path)? {
        BackupFileState::Missing => return Err(SecretBackupError::BackupNotFound),
        BackupFileState::Regular => {}
    }

    let metadata = fs::metadata(path).map_err(|_| SecretBackupError::Io)?;
    if metadata.len() < BACKUP_HEADER_BYTES as u64
        || metadata.len() > MAX_SECRET_BACKUP_BYTES as u64
    {
        return Err(SecretBackupError::InvalidBackupFormat);
    }

    fs::read(path).map_err(|_| SecretBackupError::Io)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), SecretBackupError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| SecretBackupError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), SecretBackupError> {
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
fn sync_directory(_path: &Path) -> Result<(), SecretBackupError> {
    Ok(())
}

fn map_source_vault_error(_error: VaultAccessError) -> SecretBackupError {
    SecretBackupError::SourceVaultUnavailable
}

fn map_backup_vault_format_error(error: VaultError) -> SecretBackupError {
    match error {
        VaultError::UnsupportedFormat => SecretBackupError::InvalidBackupFormat,
        VaultError::InvalidFormat | VaultError::InvalidKdfMetadata | VaultError::PayloadTooLarge => {
            SecretBackupError::InvalidBackupFormat
        }
        _ => SecretBackupError::InvalidBackupFormat,
    }
}

fn map_key_derivation_error(error: VaultAccessError) -> SecretBackupError {
    match error {
        VaultAccessError::InvalidKdfMetadata | VaultAccessError::KeyDerivationFailed => {
            SecretBackupError::InvalidBackupFormat
        }
        _ => SecretBackupError::RestoreFailed,
    }
}

fn map_restore_error(error: VaultError) -> SecretBackupError {
    match error {
        VaultError::AuthenticationFailed => SecretBackupError::BackupAuthenticationFailed,
        VaultError::UnsafeStorageLayout => SecretBackupError::RestoreTargetUnsafe,
        VaultError::InvalidFormat
        | VaultError::UnsupportedFormat
        | VaultError::InvalidKdfMetadata
        | VaultError::PayloadTooLarge => SecretBackupError::InvalidBackupFormat,
        VaultError::VerificationFailed | VaultError::RollbackFailed | VaultError::Io => {
            SecretBackupError::RestoreFailed
        }
        _ => SecretBackupError::RestoreFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_storage::VaultKdfMetadata;
    use std::time::{SystemTime, UNIX_EPOCH};

    const PASSPHRASE_A: &str = "SYNTHETIC_BACKUP_PASSPHRASE_ALPHA";
    const PASSPHRASE_B: &str = "SYNTHETIC_BACKUP_PASSPHRASE_BETA";
    const PAYLOAD_A: &[u8] = b"SYNTHETIC_SECRET_BACKUP_PAYLOAD_ALPHA";
    const PAYLOAD_B: &[u8] = b"SYNTHETIC_SECRET_BACKUP_PAYLOAD_BETA";

    fn temporary_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-secret-backup-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn passphrase(value: &str) -> MasterPassphrase {
        MasterPassphrase::new(value.to_string()).expect("passphrase")
    }

    fn metadata(seed: u8) -> VaultKdfMetadata {
        VaultKdfMetadata::argon2id([seed; 16], 8, 1, 1).expect("metadata")
    }

    fn create_test_vault(root: &Path, pass: &str, payload: &[u8], seed: u8) -> UnlockedVault {
        let kdf = metadata(seed);
        let phrase = passphrase(pass);
        let key = derive_vault_key(&phrase, kdf).expect("derive");
        drop(phrase);

        let store = VaultStore::new(root);
        store.save(&key, kdf, payload).expect("save test vault");
        UnlockedVault::from_verified_parts(store, key, kdf)
    }

    fn prepare_backup_dir() -> (PathBuf, PathBuf) {
        let root = temporary_root("backup-dir");
        fs::create_dir_all(&root).expect("backup directory");
        let path = root.join("vault.dgmb");
        (root, path)
    }

    #[test]
    fn backup_contains_ciphertext_not_plaintext() {
        let source_root = temporary_root("source");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x11);

        create_backup(&source, &backup_path).expect("backup");

        let encoded = fs::read(&backup_path).expect("read backup");
        assert_eq!(&encoded[0..8], BACKUP_MAGIC);
        assert!(!encoded.windows(PAYLOAD_A.len()).any(|window| window == PAYLOAD_A));

        drop(source);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn backup_restore_round_trip_returns_unlocked_vault() {
        let source_root = temporary_root("roundtrip-source");
        let destination_root = temporary_root("roundtrip-destination");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x21);

        create_backup(&source, &backup_path).expect("backup");
        let restored = restore_backup(
            &backup_path,
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_A),
        )
        .expect("restore");

        assert_eq!(restored.load().expect("load").expose_secret(), PAYLOAD_A);

        drop(source);
        drop(restored);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(destination_root).expect("cleanup destination");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn wrong_passphrase_does_not_replace_existing_target() {
        let source_root = temporary_root("wrong-source");
        let destination_root = temporary_root("wrong-destination");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x31);
        let destination =
            create_test_vault(&destination_root, PASSPHRASE_B, PAYLOAD_B, 0x32);

        create_backup(&source, &backup_path).expect("backup");
        drop(destination);

        let error = restore_backup(
            &backup_path,
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_B),
        )
        .expect_err("wrong passphrase must fail");
        assert_eq!(error, SecretBackupError::BackupAuthenticationFailed);

        let reopened = UnlockedVault::unlock_existing(
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_B),
        )
        .expect("original target remains");
        assert_eq!(reopened.load().expect("load target").expose_secret(), PAYLOAD_B);

        drop(source);
        drop(reopened);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(destination_root).expect("cleanup destination");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn authenticated_restore_replaces_existing_vault() {
        let source_root = temporary_root("replace-source");
        let destination_root = temporary_root("replace-destination");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x41);
        let old = create_test_vault(&destination_root, PASSPHRASE_B, PAYLOAD_B, 0x42);

        create_backup(&source, &backup_path).expect("backup");
        drop(old);

        let restored = restore_backup(
            &backup_path,
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_A),
        )
        .expect("restore");

        assert_eq!(restored.load().expect("load restored").expose_secret(), PAYLOAD_A);

        drop(source);
        drop(restored);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(destination_root).expect("cleanup destination");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn tampered_backup_is_rejected_before_target_replacement() {
        let source_root = temporary_root("tamper-source");
        let destination_root = temporary_root("tamper-destination");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x51);
        let target = create_test_vault(&destination_root, PASSPHRASE_B, PAYLOAD_B, 0x52);

        create_backup(&source, &backup_path).expect("backup");
        drop(target);

        let mut bytes = fs::read(&backup_path).expect("backup bytes");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&backup_path, bytes).expect("tamper");

        let error = restore_backup(
            &backup_path,
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_A),
        )
        .expect_err("tamper must fail");
        assert_eq!(error, SecretBackupError::BackupAuthenticationFailed);

        let reopened = UnlockedVault::unlock_existing(
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_B),
        )
        .expect("target unchanged");
        assert_eq!(reopened.load().expect("load target").expose_secret(), PAYLOAD_B);

        drop(source);
        drop(reopened);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(destination_root).expect("cleanup destination");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn create_backup_refuses_to_overwrite_existing_file() {
        let source_root = temporary_root("overwrite-source");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x61);

        fs::write(&backup_path, b"EXISTING_BACKUP_SENTINEL").expect("existing");
        let error = create_backup(&source, &backup_path).expect_err("must not overwrite");
        assert_eq!(error, SecretBackupError::BackupAlreadyExists);
        assert_eq!(
            fs::read(&backup_path).expect("existing remains"),
            b"EXISTING_BACKUP_SENTINEL"
        );

        drop(source);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn restore_refuses_ambiguous_target_layout() {
        let source_root = temporary_root("ambiguous-source");
        let destination_root = temporary_root("ambiguous-destination");
        let (backup_root, backup_path) = prepare_backup_dir();
        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x71);

        create_backup(&source, &backup_path).expect("backup");
        fs::create_dir_all(&destination_root).expect("destination");
        fs::write(destination_root.join("vault.pending.dgmv"), b"AMBIGUOUS").expect("pending");

        let error = restore_backup(
            &backup_path,
            VaultStore::new(&destination_root),
            passphrase(PASSPHRASE_A),
        )
        .expect_err("ambiguous target must fail");
        assert_eq!(error, SecretBackupError::RestoreTargetUnsafe);

        drop(source);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_dir_all(destination_root).expect("cleanup destination");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }

    #[test]
    fn errors_never_echo_passphrase_or_payload() {
        let error = SecretBackupError::BackupAuthenticationFailed;
        let message = error.to_string();
        assert_eq!(message, "secret backup authentication failed");
        assert!(!message.contains(PASSPHRASE_A));
        assert!(!message.contains(std::str::from_utf8(PAYLOAD_A).expect("utf8")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_backup_destination_is_rejected() {
        use std::os::unix::fs::symlink;

        let source_root = temporary_root("symlink-source");
        let (backup_root, backup_path) = prepare_backup_dir();
        let outside = backup_root.join("outside");
        fs::write(&outside, b"OUTSIDE").expect("outside");
        symlink(&outside, &backup_path).expect("symlink");

        let source = create_test_vault(&source_root, PASSPHRASE_A, PAYLOAD_A, 0x81);
        assert_eq!(
            create_backup(&source, &backup_path).unwrap_err(),
            SecretBackupError::UnsafeBackupLayout
        );

        drop(source);
        fs::remove_dir_all(source_root).expect("cleanup source");
        fs::remove_file(&backup_path).expect("remove symlink");
        fs::remove_file(outside).expect("remove outside");
        fs::remove_dir_all(backup_root).expect("cleanup backup");
    }
}
