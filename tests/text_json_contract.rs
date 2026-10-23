const SOURCE: &str = include_str!("../src/text_json.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const SECRET_BOUNDARY: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");

#[test]
fn text_json_core_is_in_memory_and_bounded() {
    for required in [
        "MAX_UTILITY_INPUT_BYTES",
        "MAX_UTILITY_OUTPUT_BYTES",
        "normalize_line_endings",
        "clean_text_lines",
        "validate_json",
        "pretty_json",
        "compact_json",
    ] {
        assert!(SOURCE.contains(required), "missing utility evidence: {required}");
    }

    for forbidden in [
        "std::fs",
        "OpenOptions",
        "File::",
        "write_all",
        "crate::session_state",
        "crate::vault_storage",
        "crate::clipboard_hygiene",
        "crate::server",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "TEXT / JSON core must remain in-memory and isolated: {forbidden}"
        );
    }
}

#[test]
fn json_formatting_validates_before_whitespace_only_transform() {
    assert!(SOURCE.contains("serde_json::from_str"));
    assert!(SOURCE.contains("format_json_whitespace"));
    assert!(SOURCE.contains("in_string"));
    assert!(SOURCE.contains("escaped"));
    assert!(!SOURCE.contains("serde_json::to_string"));
    assert!(!SOURCE.contains("serde_json::to_string_pretty"));
}

#[test]
fn native_ui_dispatches_ready_text_json_without_session_payload_content() {
    let entry = REGISTRY
        .split("id: ToolId::TextJson,")
        .nth(1)
        .expect("text json registry entry")
        .split("},")
        .next()
        .expect("text json registry block");

    assert!(entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::TextJson =>"));
    assert!(UI.contains("View::TextJson"));
    assert!(UI.contains("fn text_json_view"));
    assert!(UI.contains("[NORMALIZE LF]"));
    assert!(UI.contains("[CLEAN LINES]"));
    assert!(UI.contains("[VALIDATE JSON]"));
    assert!(UI.contains("[PRETTY JSON]"));
    assert!(UI.contains("[COMPACT JSON]"));
    assert!(UI.contains("[LOAD FILES SELECTION]"));
    assert!(UI.contains("Do not use this generic utility for passwords"));

    assert!(SESSION.contains("TextJson"));
    assert!(SESSION.contains("text-json"));

    for forbidden in [
        "utility_input=",
        "utility_output=",
        "json_input=",
        "json_output=",
        "text_input=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist TEXT / JSON field: {forbidden}"
        );
    }
}

#[test]
fn secret_boundary_excludes_text_json_and_roadmap_keeps_it_complete() {
    assert!(SECRET_BOUNDARY.contains(
        "MUST NOT be routed through its generic String buffers or formatting transforms"
    ));
    assert!(ROADMAP.contains("- [x] **Text / JSON utilities**"));
    assert!(ROADMAP.contains("- [x] **File comparison**"));
    assert!(ROADMAP.contains("- [x] **Local document search**"));
    assert!(ROADMAP.contains("- [x] **Repository status and safe Git operations**"));
    assert!(ROADMAP.contains(
        "- [ ] **Backup and restore workflow** — Exactly One Next Task"
    ));
}
