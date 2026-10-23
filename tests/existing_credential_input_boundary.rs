const INPUT_SOURCE: &str = include_str!("../src/existing_credential_input.rs");

#[test]
fn existing_input_module_has_no_generic_persistence_dependencies() {
    for forbidden in [
        "text_editor",
        "session_state",
        "file_browser",
        "std::fs",
        "workspace/state/autosave",
        "session.v1",
        "serde",
    ] {
        assert!(
            !INPUT_SOURCE.contains(forbidden),
            "existing credential input must not depend on generic persistence path: {forbidden}"
        );
    }
}

#[test]
fn existing_input_boundary_converges_on_credential_model() {
    assert!(INPUT_SOURCE.contains("use crate::credential::{Credential"));
    assert!(INPUT_SOURCE.contains("pub fn into_credential(self)"));
    assert!(INPUT_SOURCE.contains("Credential::new("));
}

#[test]
fn existing_input_boundary_is_redacted() {
    assert!(INPUT_SOURCE.contains("ExistingCredentialInput([REDACTED])"));
    assert!(!INPUT_SOURCE.contains("impl fmt::Display for ExistingCredentialInput"));
}
