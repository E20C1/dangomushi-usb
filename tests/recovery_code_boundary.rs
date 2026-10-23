const SOURCE: &str = include_str!("../src/recovery_codes.rs");

#[test]
fn recovery_codes_do_not_use_generic_persistence_or_clipboard() {
    for forbidden in [
        "text_editor",
        "session_state",
        "file_browser",
        "workspace/state/autosave",
        "session.v1",
        "Clipboard",
        "clipboard",
        "std::fs",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "recovery code model must not depend on generic path: {forbidden}"
        );
    }
}

#[test]
fn recovery_code_secret_types_are_redacted_and_zeroized() {
    for required in [
        "RecoveryCode([REDACTED])",
        "RecoveryCodeSet([REDACTED])",
        "RecoveryCodeVaultPayload([REDACTED])",
        "self.value.zeroize()",
        "self.bytes.zeroize()",
    ] {
        assert!(SOURCE.contains(required), "recovery boundary missing: {required}");
    }

    assert!(!SOURCE.contains("impl fmt::Display for RecoveryCode {"));
    assert!(!SOURCE.contains("impl fmt::Display for RecoveryCodeSet {"));
    assert!(!SOURCE.contains("impl fmt::Display for RecoveryCodeVaultPayload {"));
}

#[test]
fn recovery_payload_is_versioned_and_vault_typed() {
    assert!(SOURCE.contains(r#"const PAYLOAD_MAGIC: &[u8; 8] = b"DGMRC001";"#));
    assert!(SOURCE.contains("const PAYLOAD_VERSION: u16 = 1;"));
    assert!(SOURCE.contains("pub fn encode_vault_payload(&self)"));
    assert!(SOURCE.contains("pub fn from_vault_plaintext(plaintext: &VaultPlaintext)"));
}

#[test]
fn recovery_payload_has_explicit_resource_bounds() {
    assert!(SOURCE.contains("MAX_RECOVERY_CODES: usize = 128"));
    assert!(SOURCE.contains("MAX_RECOVERY_CODE_BYTES: usize = 4096"));
    assert!(SOURCE.contains("MAX_RECOVERY_CODE_PAYLOAD_BYTES: usize = 1024 * 1024"));
}
