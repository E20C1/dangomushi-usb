const SOURCE: &str = include_str!("../src/vault_unlock.rs");

#[test]
fn unlock_layer_does_not_use_generic_workspace_persistence() {
    for forbidden in [
        "text_editor",
        "session_state",
        "file_browser",
        "workspace/state/autosave",
        "session.v1",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "vault unlock must not depend on generic persistence path: {forbidden}"
        );
    }
}

#[test]
fn master_passphrase_and_unlocked_vault_are_redacted() {
    assert!(SOURCE.contains("MasterPassphrase([REDACTED])"));
    assert!(SOURCE.contains("UnlockedVault([KEY REDACTED])"));
    assert!(!SOURCE.contains("impl fmt::Display for MasterPassphrase {"));
    assert!(!SOURCE.contains("impl fmt::Display for UnlockedVault {"));
}

#[test]
fn unlock_layer_uses_argon2id_and_consuming_lock() {
    for required in [
        "Algorithm::Argon2id",
        "Version::V0x13",
        "hash_password_into",
        "pub fn lock(self) -> VaultStore",
        "drop(key);",
    ] {
        assert!(SOURCE.contains(required), "unlock contract missing: {required}");
    }
}

#[test]
fn wrong_passphrase_maps_to_generic_unlock_failure() {
    assert!(SOURCE.contains("VaultAccessError::UnlockFailed"));
    assert!(SOURCE.contains(""vault unlock failed""));
}
