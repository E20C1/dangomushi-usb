const SOURCE: &str = include_str!("../src/hash_verify.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
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
fn native_ui_dispatches_ready_hash_verify_without_persisting_tool_inputs() {
    let hash_entry = REGISTRY
        .split("id: ToolId::HashVerify,")
        .nth(1)
        .expect("hash registry entry");
    let hash_entry = hash_entry
        .split("},")
        .next()
        .expect("hash registry block");

    assert!(hash_entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::HashVerify =>"));
    assert!(UI.contains("View::HashVerify"));
    assert!(UI.contains("fn hash_verify_view"));
    assert!(UI.contains("[USE FILES SELECTION]"));
    assert!(UI.contains("[CALCULATE SHA-256]"));
    assert!(UI.contains("[VERIFY]"));

    assert!(SESSION.contains("HashVerify"));
    assert!(SESSION.contains("hash-verify"));

    for forbidden in ["hash_target=", "expected_digest=", "actual_digest="] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist HASH / VERIFY field: {forbidden}"
        );
    }
}

#[test]
fn roadmap_keeps_hash_verify_complete_after_later_m2_tools_advance() {
    assert!(ROADMAP.contains("- [x] **SHA-256 / integrity verification core**"));
    assert!(ROADMAP.contains("- [x] **HASH / VERIFY native UI integration**"));
    assert!(ROADMAP.contains("- [x] **Text / JSON utilities**"));
    assert!(ROADMAP.contains("- [x] **File comparison**"));
    assert!(ROADMAP.contains("- [x] **Local document search**"));
    assert!(ROADMAP.contains(
        "- [ ] **Repository status and safe Git operations** — Exactly One Next Task"
    ));
}
