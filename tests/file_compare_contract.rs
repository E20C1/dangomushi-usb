const SOURCE: &str = include_str!("../src/file_compare.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const SECRET_BOUNDARY: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");

#[test]
fn compare_core_is_read_only_streaming_and_workspace_rooted() {
    for required in [
        "BufReader::with_capacity",
        "fill_buf",
        "consume",
        "fs::symlink_metadata",
        "canonical_candidate.starts_with(&canonical_root)",
        "TargetNotRegularFile",
        "first_difference_byte",
        "TEXT_DETAIL_MAX_BYTES",
    ] {
        assert!(SOURCE.contains(required), "missing compare evidence: {required}");
    }

    for forbidden in [
        "OpenOptions",
        "write_all",
        "remove_file",
        "rename(",
        "crate::text_editor",
        "crate::session_state",
        "crate::vault_storage",
        "crate::server",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "COMPARE core must remain read-only and isolated: {forbidden}"
        );
    }
}

#[test]
fn native_ui_dispatches_ready_compare_without_session_payload_content() {
    let entry = REGISTRY
        .split("id: ToolId::Compare,")
        .nth(1)
        .expect("compare registry entry")
        .split("},")
        .next()
        .expect("compare registry block");

    assert!(entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::Compare =>"));
    assert!(UI.contains("View::Compare"));
    assert!(UI.contains("fn compare_view"));
    assert!(UI.contains("[COMPARE FILES]"));
    assert!(UI.contains("[USE FILES SELECTION AS A]"));
    assert!(UI.contains("[USE FILES SELECTION AS B]"));
    assert!(UI.contains("COMPARE AS A"));
    assert!(UI.contains("COMPARE AS B"));

    assert!(SESSION.contains("Compare"));
    assert!(SESSION.contains("compare"));

    for forbidden in [
        "compare_left=",
        "compare_right=",
        "first_difference=",
        "left_bytes=",
        "right_bytes=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist COMPARE field: {forbidden}"
        );
    }
}

#[test]
fn compare_stays_outside_secret_boundary_and_advances_only_to_search() {
    assert!(SECRET_BOUNDARY.contains(
        "MUST NOT be routed through its generic file-comparison path"
    ));
    assert!(ROADMAP.contains("- [x] **File comparison**"));
    assert!(ROADMAP.contains(
        "- [ ] **Local document search** — Exactly One Next Task"
    ));
}
