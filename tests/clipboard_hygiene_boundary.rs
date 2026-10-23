const SOURCE: &str = include_str!("../src/clipboard_hygiene.rs");
const CARGO: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");

#[test]
fn clipboard_secret_types_are_redacted_and_zeroized() {
    for required in [
        "pub struct ClipboardSecret",
        "ClipboardSecret([REDACTED])",
        "self.value.zeroize()",
        "pub struct ClipboardObservedText",
        "ClipboardObservedText([REDACTED])",
    ] {
        assert!(SOURCE.contains(required), "clipboard secret boundary missing: {required}");
    }
    assert!(!SOURCE.contains("impl fmt::Display for ClipboardSecret"));
    assert!(!SOURCE.contains("impl fmt::Display for ClipboardObservedText"));
}

#[test]
fn clipboard_clear_is_conditional_and_deadline_driven() {
    for required in [
        "pub const DEFAULT_CLEAR_DELAY: Duration = Duration::from_secs(30)",
        "pub struct ClipboardPolicy",
        "now < lease.clear_at",
        "observed.expose_secret() == expected",
        "ClipboardPoll::PreservedExternalContent",
        "self.backend.clear()?",
    ] {
        assert!(SOURCE.contains(required), "clipboard timing contract missing: {required}");
    }
}

#[test]
fn clipboard_core_remains_outside_generic_persistence() {
    for forbidden in [
        "crate::text_editor",
        "crate::session_state",
        "crate::file_browser",
        "crate::workspace",
        "write_recovery",
        "save_text",
        "session.v1",
    ] {
        assert!(!SOURCE.contains(forbidden), "clipboard boundary must not use generic persistence: {forbidden}");
    }
}

#[test]
fn real_os_clipboard_is_desktop_feature_gated() {
    assert!(LIB.contains("pub mod clipboard_hygiene;"));
    assert!(CARGO.contains(r#"desktop = ["dep:eframe", "dep:arboard"]"#));
    assert!(CARGO.contains(
        r#"arboard = { version = "3.7.0", optional = true, default-features = false, features = ["wayland-data-control"] }"#
    ));
    assert!(SOURCE.contains(r#"#[cfg(feature = "desktop")]"#));
    assert!(SOURCE.contains("pub struct SystemClipboard"));
}

#[test]
fn supported_secret_sources_are_explicit() {
    for required in [
        "from_generated_password",
        "from_credential_password",
        "from_recovery_code",
    ] {
        assert!(SOURCE.contains(required), "missing explicit clipboard source: {required}");
    }
}
