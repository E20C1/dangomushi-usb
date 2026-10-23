use crate::vault_storage::{
    VaultError, VaultKdfMetadata, VaultKey, VaultPlaintext, VaultStore,
};
use argon2::{Algorithm, Argon2, Params, Version};
use std::error::Error;
use std::fmt;
use std::path::Path;
use zeroize::Zeroize;

pub const DEFAULT_KDF_MEMORY_KIB: u32 = 19 * 1024;
pub const DEFAULT_KDF_ITERATIONS: u32 = 2;
pub const DEFAULT_KDF_PARALLELISM: u32 = 1;

#[must_use]
pub struct MasterPassphrase {
    bytes: Vec<u8>,
}

impl MasterPassphrase {
    pub fn new(value: String) -> Result<Self, VaultAccessError> {
        if value.is_empty() {
            return Err(VaultAccessError::EmptyMasterPassphrase);
        }

        Ok(Self {
            bytes: value.into_bytes(),
        })
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for MasterPassphrase {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for MasterPassphrase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MasterPassphrase([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultKdfProfile {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

impl Default for VaultKdfProfile {
    fn default() -> Self {
        Self {
            memory_kib: DEFAULT_KDF_MEMORY_KIB,
            iterations: DEFAULT_KDF_ITERATIONS,
            parallelism: DEFAULT_KDF_PARALLELISM,
        }
    }
}

impl VaultKdfProfile {
    pub fn memory_kib(&self) -> u32 {
        self.memory_kib
    }

    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    pub fn parallelism(&self) -> u32 {
        self.parallelism
    }

    fn generate_metadata(&self) -> Result<VaultKdfMetadata, VaultAccessError> {
        let mut salt = [0_u8; 16];
        getrandom::fill(&mut salt).map_err(|_| VaultAccessError::RandomSourceUnavailable)?;

        VaultKdfMetadata::argon2id(
            salt,
            self.memory_kib,
            self.iterations,
            self.parallelism,
        )
        .map_err(map_metadata_error)
    }

    #[cfg(test)]
    fn test_profile() -> Self {
        Self {
            memory_kib: 8,
            iterations: 1,
            parallelism: 1,
        }
    }
}

#[must_use]
pub struct UnlockedVault {
    store: VaultStore,
    key: VaultKey,
    kdf: VaultKdfMetadata,
}

impl UnlockedVault {
    pub fn initialize_new(
        store: VaultStore,
        passphrase: MasterPassphrase,
        initial_plaintext: &[u8],
    ) -> Result<Self, VaultAccessError> {
        match store.read_kdf_metadata() {
            Ok(_) => return Err(VaultAccessError::VaultAlreadyExists),
            Err(VaultError::VaultNotFound) => {}
            Err(error) => return Err(map_storage_error(error)),
        }

        Self::initialize_new_with_profile(
            store,
            passphrase,
            VaultKdfProfile::default(),
            initial_plaintext,
        )
    }

    pub fn unlock_existing(
        store: VaultStore,
        passphrase: MasterPassphrase,
    ) -> Result<Self, VaultAccessError> {
        let kdf = store
            .read_kdf_metadata()
            .map_err(map_storage_error)?;

        let key = derive_vault_key(&passphrase, kdf)?;
        drop(passphrase);

        let verification = store.load(&key).map_err(map_unlock_error)?;
        drop(verification);

        Ok(Self { store, key, kdf })
    }

    pub fn load(&self) -> Result<VaultPlaintext, VaultAccessError> {
        self.store.load(&self.key).map_err(map_storage_error)
    }

    pub fn save(&self, plaintext: &[u8]) -> Result<(), VaultAccessError> {
        self.store
            .save(&self.key, self.kdf, plaintext)
            .map_err(map_storage_error)
    }

    pub fn kdf_metadata(&self) -> VaultKdfMetadata {
        self.kdf
    }

    pub fn store_root(&self) -> &Path {
        self.store.root()
    }

    pub fn lock(self) -> VaultStore {
        let Self { store, key, .. } = self;
        drop(key);
        store
    }

    pub(crate) fn export_verified_ciphertext(&self) -> Result<Vec<u8>, VaultAccessError> {
        self.store
            .export_verified_ciphertext(&self.key)
            .map_err(map_storage_error)
    }

    pub(crate) fn from_verified_parts(
        store: VaultStore,
        key: VaultKey,
        kdf: VaultKdfMetadata,
    ) -> Self {
        Self { store, key, kdf }
    }

    fn initialize_new_with_profile(
        store: VaultStore,
        passphrase: MasterPassphrase,
        profile: VaultKdfProfile,
        initial_plaintext: &[u8],
    ) -> Result<Self, VaultAccessError> {
        let kdf = profile.generate_metadata()?;
        let key = derive_vault_key(&passphrase, kdf)?;
        drop(passphrase);

        store
            .save(&key, kdf, initial_plaintext)
            .map_err(map_storage_error)?;

        Ok(Self { store, key, kdf })
    }
}

impl fmt::Debug for UnlockedVault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UnlockedVault([KEY REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultAccessError {
    EmptyMasterPassphrase,
    VaultAlreadyExists,
    VaultNotFound,
    InvalidKdfMetadata,
    RandomSourceUnavailable,
    KeyDerivationFailed,
    UnlockFailed,
    UnsupportedVaultFormat,
    Storage,
}

impl fmt::Display for VaultAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyMasterPassphrase => "master passphrase must not be empty",
            Self::VaultAlreadyExists => "encrypted vault already exists",
            Self::VaultNotFound => "encrypted vault not found",
            Self::InvalidKdfMetadata => "vault key-derivation metadata is invalid",
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
            Self::KeyDerivationFailed => "vault key derivation failed",
            Self::UnlockFailed => "vault unlock failed",
            Self::UnsupportedVaultFormat => "vault format is unsupported",
            Self::Storage => "vault storage operation failed",
        };

        formatter.write_str(message)
    }
}

impl Error for VaultAccessError {}

pub(crate) fn derive_vault_key(
    passphrase: &MasterPassphrase,
    metadata: VaultKdfMetadata,
) -> Result<VaultKey, VaultAccessError> {
    let params = Params::new(
        metadata.memory_kib(),
        metadata.iterations(),
        metadata.parallelism(),
        Some(32),
    )
    .map_err(|_| VaultAccessError::InvalidKdfMetadata)?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut derived = [0_u8; 32];

    if argon2
        .hash_password_into(passphrase.as_bytes(), metadata.salt(), &mut derived)
        .is_err()
    {
        derived.zeroize();
        return Err(VaultAccessError::KeyDerivationFailed);
    }

    let key = VaultKey::from_derived_bytes(&mut derived);
    debug_assert!(derived.iter().all(|byte| *byte == 0));
    Ok(key)
}

fn map_metadata_error(error: VaultError) -> VaultAccessError {
    match error {
        VaultError::InvalidKdfMetadata => VaultAccessError::InvalidKdfMetadata,
        _ => VaultAccessError::Storage,
    }
}

fn map_unlock_error(error: VaultError) -> VaultAccessError {
    match error {
        VaultError::AuthenticationFailed | VaultError::VerificationFailed => {
            VaultAccessError::UnlockFailed
        }
        VaultError::VaultNotFound => VaultAccessError::VaultNotFound,
        VaultError::InvalidKdfMetadata => VaultAccessError::InvalidKdfMetadata,
        VaultError::UnsupportedFormat => VaultAccessError::UnsupportedVaultFormat,
        VaultError::InvalidFormat => VaultAccessError::UnlockFailed,
        VaultError::RandomSourceUnavailable => VaultAccessError::RandomSourceUnavailable,
        _ => VaultAccessError::Storage,
    }
}

fn map_storage_error(error: VaultError) -> VaultAccessError {
    match error {
        VaultError::VaultNotFound => VaultAccessError::VaultNotFound,
        VaultError::InvalidKdfMetadata => VaultAccessError::InvalidKdfMetadata,
        VaultError::RandomSourceUnavailable => VaultAccessError::RandomSourceUnavailable,
        VaultError::UnsupportedFormat => VaultAccessError::UnsupportedVaultFormat,
        VaultError::AuthenticationFailed | VaultError::VerificationFailed => {
            VaultAccessError::UnlockFailed
        }
        _ => VaultAccessError::Storage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    const PASSPHRASE_SENTINEL: &str = "SYNTHETIC_MASTER_PASSPHRASE";
    const WRONG_PASSPHRASE_SENTINEL: &str = "SYNTHETIC_WRONG_MASTER_PASSPHRASE";
    const PAYLOAD: &[u8] = b"SYNTHETIC_UNLOCK_PAYLOAD";

    fn temporary_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-unlock-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn passphrase(value: &str) -> MasterPassphrase {
        MasterPassphrase::new(value.to_string()).expect("master passphrase")
    }

    fn initialize_test_vault(root: &Path) -> UnlockedVault {
        UnlockedVault::initialize_new_with_profile(
            VaultStore::new(root),
            passphrase(PASSPHRASE_SENTINEL),
            VaultKdfProfile::test_profile(),
            PAYLOAD,
        )
        .expect("initialize vault")
    }

    #[test]
    fn default_kdf_profile_matches_portable_baseline() {
        let profile = VaultKdfProfile::default();

        assert_eq!(profile.memory_kib(), 19 * 1024);
        assert_eq!(profile.iterations(), 2);
        assert_eq!(profile.parallelism(), 1);
    }

    #[test]
    fn master_passphrase_debug_is_redacted() {
        let value = passphrase(PASSPHRASE_SENTINEL);
        let debug = format!("{value:?}");

        assert_eq!(debug, "MasterPassphrase([REDACTED])");
        assert!(!debug.contains(PASSPHRASE_SENTINEL));
    }

    #[test]
    fn empty_master_passphrase_is_rejected_without_echo() {
        let error = MasterPassphrase::new(String::new()).expect_err("empty must fail");

        assert_eq!(error, VaultAccessError::EmptyMasterPassphrase);
        assert_eq!(error.to_string(), "master passphrase must not be empty");
    }

    #[test]
    fn initialize_lock_and_unlock_round_trip() {
        let root = temporary_root("roundtrip");

        let unlocked = initialize_test_vault(&root);
        assert_eq!(unlocked.load().expect("load").expose_secret(), PAYLOAD);

        let store = unlocked.lock();
        let reopened =
            UnlockedVault::unlock_existing(store, passphrase(PASSPHRASE_SENTINEL))
                .expect("unlock");

        assert_eq!(reopened.load().expect("reload").expose_secret(), PAYLOAD);

        drop(reopened);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn wrong_master_passphrase_returns_generic_unlock_failure() {
        let root = temporary_root("wrong-passphrase");

        let unlocked = initialize_test_vault(&root);
        let store = unlocked.lock();

        let error = UnlockedVault::unlock_existing(
            store,
            passphrase(WRONG_PASSPHRASE_SENTINEL),
        )
        .expect_err("wrong passphrase must fail");

        assert_eq!(error, VaultAccessError::UnlockFailed);
        assert_eq!(error.to_string(), "vault unlock failed");
        assert!(!error.to_string().contains(WRONG_PASSPHRASE_SENTINEL));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unlocked_vault_debug_does_not_expose_root_or_key() {
        let root = temporary_root("debug");
        let unlocked = initialize_test_vault(&root);
        let debug = format!("{unlocked:?}");

        assert_eq!(debug, "UnlockedVault([KEY REDACTED])");
        assert!(!debug.contains(root.to_string_lossy().as_ref()));

        drop(unlocked);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unlocked_vault_can_save_with_derived_key() {
        let root = temporary_root("save");
        let unlocked = initialize_test_vault(&root);

        unlocked
            .save(b"SYNTHETIC_UNLOCK_PAYLOAD_SECOND")
            .expect("save");

        assert_eq!(
            unlocked.load().expect("load").expose_secret(),
            b"SYNTHETIC_UNLOCK_PAYLOAD_SECOND"
        );

        drop(unlocked);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn initializing_over_existing_vault_is_rejected() {
        let root = temporary_root("already-exists");
        let unlocked = initialize_test_vault(&root);
        let store = unlocked.lock();

        let error = UnlockedVault::initialize_new(
            store,
            passphrase(PASSPHRASE_SENTINEL),
            PAYLOAD,
        )
        .expect_err("existing vault must not be overwritten");

        assert_eq!(error, VaultAccessError::VaultAlreadyExists);

        fs::remove_dir_all(root).expect("cleanup");
    }
}
