const CONTRACT: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");

#[test]
fn secret_boundary_contract_keeps_generic_persistence_out_of_scope() {
    for required in [
        "MUST NOT be routed through the normal Text Editor",
        "MUST NOT be written to the generic recovery drafts",
        "workspace/state/session.v1",
        "MUST NOT be persisted as ordinary plaintext Workspace JSON, TOML, CSV",
        "MUST NOT appear in `Debug`, `Display`, error messages",
        "dedicated secret-storage API boundary",
        "Cryptography MUST NOT be invented inside DANGOMUSHI",
        "MUST NOT claim guaranteed complete memory erasure",
    ] {
        assert!(
            CONTRACT.contains(required),
            "secret boundary contract is missing required clause: {required}"
        );
    }
}

#[test]
fn roadmap_keeps_secret_work_separate_and_generator_next() {
    assert!(ROADMAP.contains("## M2 — Local tools"));
    assert!(ROADMAP.contains("## M2-S — Secret management subsystem"));
    assert!(ROADMAP.contains("Secret Data Boundary"));
    assert!(ROADMAP.contains("Password Generator Core"));
    assert!(ROADMAP.contains("Exactly One Next Task"));
    assert!(ROADMAP.contains("Add Existing Credential input boundary"));
}

#[test]
fn boundary_requires_generated_and_existing_passwords_to_converge() {
    assert!(CONTRACT.contains("Generate new password"));
    assert!(CONTRACT.contains("Enter existing password"));
    assert!(CONTRACT.contains("same Credential persistence flow"));
}
