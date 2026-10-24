const SOURCE: &str = include_str!("../src/document_search.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const SECRET_BOUNDARY: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");

#[test]
fn document_search_core_is_read_only_bounded_and_documents_scoped() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        "DOCUMENTS_RELATIVE",
        "[\"data\", \"documents\"]",
        "MAX_SEARCH_QUERY_BYTES",
        "MAX_DOCUMENT_BYTES",
        "MAX_FILES_CONSIDERED",
        "MAX_SEARCH_RESULTS",
        "MAX_PREVIEW_CHARS",
        "fs::symlink_metadata",
        "canonical.starts_with(&documents_root)",
        "File::open",
        "file.take(MAX_DOCUMENT_BYTES + 1)",
        "String::from_utf8",
    ] {
        assert!(
            production.contains(required),
            "missing search boundary evidence: {required}"
        );
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
            !production.contains(forbidden),
            "SEARCH core must remain read-only and isolated: {forbidden}"
        );
    }
}

#[test]
fn native_ui_dispatches_ready_search_without_session_payload_content() {
    let entry = REGISTRY
        .split("id: ToolId::Search,")
        .nth(1)
        .expect("search registry entry")
        .split("},")
        .next()
        .expect("search registry block");

    assert!(entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::Search =>"));
    assert!(UI.contains("View::Search"));
    assert!(UI.contains("fn search_view"));
    assert!(UI.contains("[SEARCH DOCUMENTS]"));
    assert!(UI.contains("[OPEN]"));
    assert!(UI.contains("workspace:/data/documents"));

    assert!(SESSION.contains("Search"));
    assert!(SESSION.contains("search"));

    for forbidden in [
        "search_query=",
        "search_results=",
        "search_preview=",
        "search_line=",
        "search_column=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist SEARCH field: {forbidden}"
        );
    }
}

#[test]
fn search_stays_outside_secret_boundary_after_repositories_advance() {
    assert!(SECRET_BOUNDARY.contains(
        "MUST NOT be routed through its generic document-search path or result previews"
    ));
    assert!(ROADMAP.contains("- [x] **Local document search**"));
    assert!(ROADMAP.contains("- [x] **Repository status and safe Git operations**"));
    assert!(ROADMAP.contains("- [x] **Backup and restore workflow**"));
    assert!(ROADMAP.contains("- [x] **Explicit LAN enable control**"));
    assert!(ROADMAP.contains("- [x] **Device pairing**"));
    assert!(ROADMAP.contains(
        "- [ ] **Persistent trusted-device public keys** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains("- [x] **Repository status and safe Git operations**"));
}
