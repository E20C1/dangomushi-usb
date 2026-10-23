const SOURCE: &str = include_str!("../src/generated_credential_input.rs");
const GENERATOR: &str = include_str!("../src/password_generator.rs");

#[test]
fn generated_input_has_no_generic_persistence_dependencies() {
    for forbidden in [
        "text_editor",
        "session_state",
        "file_browser",
        "workspace/state/autosave",
        "session.v1",
        "VaultStore",
        "UnlockedVault",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "generated credential input must not depend on unrelated persistence path: {forbidden}"
        );
    }
}

#[test]
fn generated_input_converges_on_source_agnostic_credential() {
    assert!(SOURCE.contains("use crate::credential::{Credential"));
    assert!(SOURCE.contains("GeneratedPassword"));
    assert!(SOURCE.contains("pub fn into_credential(self)"));
    assert!(SOURCE.contains("Credential::new("));
}

#[test]
fn production_path_consumes_generated_password_without_public_exposure() {
    assert!(SOURCE.contains("self.password.into_owned_secret()"));
    assert!(!SOURCE.contains("self.password.expose_secret()"));
    assert!(GENERATOR.contains("pub(crate) fn into_owned_secret(self) -> String"));
}

#[test]
fn generated_input_is_redacted() {
    assert!(SOURCE.contains("GeneratedCredentialInput([REDACTED])"));
    assert!(!SOURCE.contains("impl fmt::Display for GeneratedCredentialInput {"));
}
