const SOURCE: &str = include_str!("../src/hash_verify.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");

#[test]
fn hash_verify_core_stays_read_only_and_workspace_rooted() {
    for required in [
        "validate_relative_path",
        "fs::symlink_metadata",
        "canonical_candidate.starts_with(&canonical_root)",
        "TargetNotRegularFile",
        "File::open",
        "file.read",
    ] {
        assert!(SOURCE.contains(required), "missing boundary evidence: {required}");
    }

    for forbidden in [
        "OpenOptions",
        "write_all",
        "rename(",
        "crate::text_editor",
        "crate::session_state",
        "crate::vault_storage",
        "crate::server",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "HASH / VERIFY core must remain read-only and isolated: {forbidden}"
        );
    }
}

#[test]
fn hash_verify_core_is_sha256_only_for_this_task() {
    assert!(SOURCE.contains("Sha256"));
    assert!(!SOURCE.contains("Sha512"));
    assert!(!SOURCE.contains("Blake"));
    assert!(SOURCE.contains("SHA256_HEX_LENGTH"));
}

#[test]
fn roadmap_keeps_ui_as_the_next_hash_verify_step() {
    assert!(ROADMAP.contains("- [x] **SHA-256 / integrity verification core**"));
    assert!(ROADMAP.contains(
        "- [ ] **HASH / VERIFY native UI integration** — Exactly One Next Task"
    ));
}
