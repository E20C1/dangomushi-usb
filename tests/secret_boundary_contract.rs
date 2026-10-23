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
fn roadmap_completes_secret_core_and_keeps_non_secret_m2_sequence_bounded() {
    assert!(ROADMAP.contains("## M2 — Local tools"));
    assert!(ROADMAP.contains("## M2-S — Secret management subsystem"));
    assert!(ROADMAP.contains("[x] **Password Generator Core**"));
    assert!(ROADMAP.contains("[x] **Credential model**"));
    assert!(ROADMAP.contains("[x] **Add Existing Credential input boundary**"));
    assert!(ROADMAP.contains("[x] **Encrypted Vault storage**"));
    assert!(ROADMAP.contains("[x] **Vault unlock / lock**"));
    assert!(ROADMAP.contains("[x] **Generator -> Credential integration**"));
    assert!(ROADMAP.contains("[x] **Recovery Codes**"));
    assert!(ROADMAP.contains("[x] **Clipboard hygiene**"));
    assert!(ROADMAP.contains("[x] **Secret Backup / Restore**"));
    assert!(ROADMAP.contains("[x] **SHA-256 / integrity verification core**"));
    assert!(ROADMAP.contains("[x] **HASH / VERIFY native UI integration**"));
    assert!(ROADMAP.contains("[x] **Text / JSON utilities**"));
    assert!(ROADMAP.contains("[x] **File comparison**"));
    assert!(ROADMAP.contains("[x] **Local document search**"));
    assert!(ROADMAP.contains(
        "[ ] **Repository status and safe Git operations** — Exactly One Next Task"
    ));
}

#[test]
fn boundary_requires_generated_and_existing_passwords_to_converge() {
    assert!(CONTRACT.contains("Generate new password"));
    assert!(CONTRACT.contains("Enter existing password"));
    assert!(CONTRACT.contains("same Credential persistence flow"));
}
