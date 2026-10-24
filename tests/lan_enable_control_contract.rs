use dangomushi::lan_access_control::{LanEnableControl, LanEnableState};

const SOURCE: &str = include_str!("../src/lan_access_control.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const SERVER: &str = include_str!("../src/server.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const BACKUP: &str = include_str!("../src/workspace_backup.rs");

#[test]
fn lan_control_defaults_off_and_requires_explicit_process_local_transition() {
    let mut control = LanEnableControl::default();

    assert_eq!(control.state(), LanEnableState::Disabled);
    assert!(!control.is_enabled());

    assert!(control.enable_for_this_run());
    assert!(control.is_enabled());

    assert!(control.disable());
    assert!(!control.is_enabled());

    let restarted = LanEnableControl::default();
    assert!(!restarted.is_enabled());
}

#[test]
fn lan_control_core_has_no_persistence_or_network_side_effect_surface() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for forbidden in [
        "std::fs",
        "File::",
        "OpenOptions",
        "session_state",
        "workspace_backup",
        "TcpListener",
        "TcpStream",
        "std::net",
        "Command::new",
    ] {
        assert!(
            !production.contains(forbidden),
            "LAN control must remain a process-local intent gate: {forbidden}"
        );
    }

    for forbidden in [
        "lan_enabled=",
        "lan_enable=",
        "lan_control=",
        "network_enabled=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist LAN control state: {forbidden}"
        );
    }

    assert!(!BACKUP.contains(r#"&["state", "session.v1"]"#));
}

#[test]
fn native_mobile_surface_is_explicit_but_does_not_unlock_listener() {
    for required in [
        "LanEnableControl",
        "ENABLE FOR THIS RUN",
        "DISABLE LAN CONTROL",
        "resets OFF on every launch",
        "does not create a LAN listener",
        "LISTENER STILL LOCKED",
    ] {
        assert!(UI.contains(required), "missing explicit LAN UI evidence: {required}");
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(SERVER.contains("is_loopback()"));
    assert!(!SERVER.contains("0.0.0.0"));
    assert!(!SERVER.contains("UPnP"));
    assert!(!SERVER.contains("NAT-PMP"));
}

#[test]
fn architecture_and_roadmap_keep_listener_blocked_and_advance_to_pairing() {
    for required in [
        "operator-intent gate, not a network-listener authorization",
        "not written to `workspace/state/session.v1`",
        "src/server.rs remains bound to `127.0.0.1`",
        "A future LAN listener must treat this explicit control as necessary operator intent",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing architecture boundary: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Explicit LAN enable control**"));
    assert!(ROADMAP.contains("- [x] **Device pairing**"));
    assert!(ROADMAP.contains(
        "- [x] **Mobile editor/session handoff**"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
