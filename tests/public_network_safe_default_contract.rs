use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::network_safety::{
    classify_connected_categories, evaluate, ConnectedNetworkCategory, LanExposureDecision,
    NetworkEnvironment, NetworkSafetySource,
};

const SOURCE: &str = include_str!("../src/network_safety.rs");
const SERVER: &str = include_str!("../src/server.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/PUBLIC_NETWORK_SAFE_DEFAULT.md");

#[test]
fn public_unknown_and_ambiguous_networks_fail_closed() {
    let public = classify_connected_categories(
        &[
            ConnectedNetworkCategory::Private,
            ConnectedNetworkCategory::Public,
        ],
        NetworkSafetySource::WindowsNetworkListManager,
    );
    let unknown = classify_connected_categories(
        &[
            ConnectedNetworkCategory::Private,
            ConnectedNetworkCategory::Unknown,
        ],
        NetworkSafetySource::WindowsNetworkListManager,
    );
    let none = classify_connected_categories(&[], NetworkSafetySource::ProbeFailed);

    assert_eq!(public.environment(), NetworkEnvironment::Public);
    assert_eq!(unknown.environment(), NetworkEnvironment::Unknown);
    assert_eq!(none.environment(), NetworkEnvironment::Unknown);

    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();

    assert_eq!(
        evaluate(&lan, public),
        LanExposureDecision::BlockedPublicNetwork
    );
    assert_eq!(
        evaluate(&lan, unknown),
        LanExposureDecision::BlockedUnknownNetwork
    );
    assert_eq!(
        evaluate(&lan, none),
        LanExposureDecision::BlockedUnknownNetwork
    );
}

#[test]
fn trusted_network_still_requires_explicit_lan_intent() {
    let trusted = classify_connected_categories(
        &[
            ConnectedNetworkCategory::Private,
            ConnectedNetworkCategory::DomainAuthenticated,
        ],
        NetworkSafetySource::WindowsNetworkListManager,
    );

    let mut lan = LanEnableControl::default();
    assert_eq!(evaluate(&lan, trusted), LanExposureDecision::LanIntentOff);

    lan.enable_for_this_run();
    assert_eq!(
        evaluate(&lan, trusted),
        LanExposureDecision::EligibleTrustedNetwork
    );
}

#[test]
fn windows_probe_uses_os_categories_and_never_changes_them() {
    for required in [
        "NLM_ENUM_NETWORK_CONNECTED",
        "NLM_NETWORK_CATEGORY_PUBLIC",
        "NLM_NETWORK_CATEGORY_PRIVATE",
        "NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED",
        "GetNetworks",
        "GetCategory",
        "CoInitializeEx",
        "CoCreateInstance",
    ] {
        assert!(SOURCE.contains(required), "missing Windows NLM evidence: {required}");
    }

    for forbidden in [
        "SetCategory",
        "powershell",
        "Get-NetConnectionProfile",
        "Command::new",
        "0.0.0.0",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "network safety probe must be read-only and non-shelling: {forbidden}"
        );
    }
}

#[test]
fn safety_state_is_ephemeral_and_server_remains_loopback_only() {
    for forbidden in [
        "network_safety=",
        "network_category=",
        "public_network=",
        "trusted_network=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist network-safety state: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
    assert!(!SERVER.contains("network_safety::"));
}

#[test]
fn native_ui_exposes_snapshot_and_fail_closed_decision() {
    for required in [
        "NETWORK SAFETY",
        "REFRESH NETWORK SAFETY",
        "LAN EXPOSURE",
        "environment().label()",
        "source().label()",
    ] {
        assert!(UI.contains(required), "missing native safety UI: {required}");
    }

    for required in [
        "PUBLIC / UNTRUSTED",
        "PRIVATE OR DOMAIN / TRUSTED",
        "BLOCKED / NETWORK UNKNOWN",
    ] {
        assert!(
            SOURCE.contains(required),
            "missing network-safety label: {required}"
        );
    }
}

#[test]
fn architecture_and_roadmap_advance_exactly_to_mobile_handoff() {
    for required in [
        "Public-network safe default",
        "all connected networks",
        "macOS and Linux",
        "immediately before binding",
        "before accepting or authorizing each remote connection",
        "cached earlier Trusted result",
        "does not create a LAN listener",
    ] {
        assert!(
            ARCHITECTURE.contains(required) || DOC.contains(required),
            "missing safe-default architecture clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Public-network safe default**"));
    assert!(ROADMAP.contains(
        "- [ ] **Mobile editor/session handoff** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
