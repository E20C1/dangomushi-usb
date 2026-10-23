const SOURCE: &str = include_str!("../src/vault_storage.rs");

#[test]
fn vault_storage_does_not_use_generic_workspace_persistence() {
    for forbidden in [
        "text_editor",
        "session_state",
        "file_browser",
        "workspace/state/autosave",
        "session.v1",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "vault storage must not depend on generic persistence path: {forbidden}"
        );
    }
}

#[test]
fn vault_storage_uses_fixed_dedicated_filenames() {
    assert!(SOURCE.contains(r#"const CURRENT_FILE: &str = "vault.dgmv";"#));
    assert!(SOURCE.contains(r#"const PREVIOUS_FILE: &str = "vault.previous.dgmv";"#));
    assert!(SOURCE.contains(r#"const PENDING_FILE: &str = "vault.pending.dgmv";"#));
}

#[test]
fn vault_storage_has_authenticated_replace_sequence() {
    for required in [
        "XChaCha20Poly1305",
        "Payload {",
        "file.sync_all()",
        "decrypt_file(&self.pending_path(), key)",
        "fs::rename(&current, &previous)",
        "fs::rename(&pending, &current)",
        "rollback_after_failed_replace",
    ] {
        assert!(
            SOURCE.contains(required),
            "vault storage contract is missing: {required}"
        );
    }
}

#[test]
fn vault_secret_types_are_redacted() {
    assert!(SOURCE.contains("VaultKey([REDACTED])"));
    assert!(SOURCE.contains("VaultPlaintext([REDACTED])"));
    assert!(!SOURCE.contains("impl fmt::Display for VaultKey {"));
    assert!(!SOURCE.contains("impl fmt::Display for VaultPlaintext {"));
}
