const SOURCE: &str = include_str!("../src/secret_backup.rs");
const VAULT_SOURCE: &str = include_str!("../src/vault_storage.rs");

#[test]
fn secret_backup_never_uses_generic_workspace_persistence() {
    for forbidden in [
        "crate::text_editor",
        "crate::session_state",
        "crate::file_browser",
        "crate::workspace",
        "write_recovery",
        "save_text",
        "session.v1",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "secret backup must not use generic persistence: {forbidden}"
        );
    }
}

#[test]
fn backup_wraps_existing_authenticated_vault_ciphertext() {
    for required in [
        "DGMBAK01",
        "export_verified_ciphertext",
        "kdf_metadata_from_encoded",
        "restore_verified_ciphertext",
        "derive_vault_key",
    ] {
        assert!(SOURCE.contains(required) || VAULT_SOURCE.contains(required));
    }

    assert!(!SOURCE.contains("XChaCha20Poly1305"));
    assert!(!SOURCE.contains("encrypt("));
}

#[test]
fn restore_authenticates_before_transactional_replace() {
    let decrypt = VAULT_SOURCE
        .find("let expected = decrypt_encoded(encoded, key)?;")
        .expect("restore must authenticate ciphertext");
    let root = VAULT_SOURCE[decrypt..]
        .find("ensure_storage_root(&self.root)?;")
        .map(|offset| decrypt + offset)
        .expect("restore must prepare target after authentication");

    assert!(decrypt < root);
    assert!(VAULT_SOURCE.contains("self.commit_pending(key, expected.expose_secret())"));
}

#[test]
fn backup_creation_is_create_new_and_syncs_data() {
    assert!(SOURCE.contains("create_new(true)"));
    assert!(SOURCE.contains("file.sync_all()"));
    assert!(SOURCE.contains(".pending"));
    assert!(SOURCE.contains("BackupAlreadyExists"));
}
