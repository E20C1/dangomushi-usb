use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::peer_lan::{self, PeerLanError, DEFAULT_PEER_LAN_PORT};
use dangomushi::peer_transfer::TransferPayload;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;
use std::time::Duration;

const LAN_SOURCE: &str = include_str!("../src/peer_lan.rs");
const TRANSFER_SOURCE: &str = include_str!("../src/peer_transfer.rs");
const NETWORK_SAFETY: &str = include_str!("../src/network_safety.rs");
const SERVER: &str = include_str!("../src/server.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/DIRECT_LAN_PEER_PATH.md");
const UI: &str = include_str!("../src/native_ui.rs");
const CLI: &str = include_str!("../src/main.rs");

fn enabled_lan() -> LanEnableControl {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    lan
}

#[test]
fn direct_lan_transport_composes_live_safety_trust_crypto_and_file_verification() {
    for required in [
        "network_safety::current_exposure_decision",
        "lan_control.is_enabled()",
        "trusted_peers::load",
        "peer_transfer::begin_receive",
        "peer_transfer::seal_transfer_to",
        "peer_transfer::open_transfer",
        ".verify()",
        "drop(listener)",
        "MAX_RECEIVE_WAIT",
        "IO_TIMEOUT",
    ] {
        assert!(
            LAN_SOURCE.contains(required),
            "missing direct-LAN security composition: {required}"
        );
    }

    assert!(TRANSFER_SOURCE.contains("pub fn seal_transfer_to("));
    assert!(TRANSFER_SOURCE.contains("expected_recipient_fingerprint"));
    assert!(NETWORK_SAFETY.contains("current_exposure_decision"));
}

#[test]
fn only_peer_lan_transport_opens_nonloopback_socket_and_web_gateway_stays_loopback() {
    assert!(LAN_SOURCE.contains(
        "TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))"
    ));
    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(SERVER.contains("is_loopback()"));
    assert!(!SERVER.contains("0.0.0.0"));
    assert!(!SERVER.contains("peer_lan::"));
    assert!(CLI.contains("the web gateway binds to 127.0.0.1 only"));
}

#[test]
fn direct_lan_rejects_global_ipv4_before_any_connection_attempt() {
    let error = peer_lan::send_transfer(
        Path::new("."),
        &enabled_lan(),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(8, 8, 8, 8), DEFAULT_PEER_LAN_PORT)),
        &[0_u8; 32],
        TransferPayload::Text("not sent".to_string()),
    )
    .unwrap_err();

    assert_eq!(error, PeerLanError::UnsupportedAddress);
}

#[cfg(not(windows))]
#[test]
fn production_direct_lan_fails_closed_on_current_unsupported_platforms() {
    let error = peer_lan::receive_one(
        Path::new("."),
        &enabled_lan(),
        DEFAULT_PEER_LAN_PORT,
        Duration::ZERO,
    )
    .unwrap_err();

    assert_eq!(error, PeerLanError::NetworkExposureBlocked);
}

#[test]
fn transport_has_no_firewall_discovery_relay_or_persistence_surface() {
    for forbidden in [
        "Command::new",
        "UPnP",
        "NAT-PMP",
        "natpmp",
        "igd::",
        "multicast",
        "broadcast",
        "relay",
        "rendezvous",
        "session_state::",
        "workspace_backup",
        "secret_backup",
        "fs::write",
        "File::create",
        "OpenOptions",
    ] {
        assert!(
            !LAN_SOURCE.contains(forbidden),
            "direct LAN core must stay narrow: {forbidden}"
        );
    }

    for forbidden in [
        "peer_lan_address=",
        "peer_lan_envelope=",
        "peer_lan_message=",
        "peer_lan_file=",
    ] {
        assert!(!SESSION.contains(forbidden));
    }
}

#[test]
fn docs_ui_and_roadmap_bound_direct_lan_before_internet_p2p() {
    for required in [
        "one-shot",
        "five minutes",
        "IPv4",
        "global",
        "seal_transfer_to",
        "VerifiedReceivedFile",
        "Internet P2P",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing direct-LAN documentation: {required}"
        );
    }

    assert!(UI.contains("DIRECT PEER LAN READY / EXPLICIT ONE-SHOT"));
    assert!(UI.contains("MOBILE LISTENER STILL LOCKED"));
    assert!(ROADMAP.contains("- [x] **Direct LAN path first**"));
    assert!(ROADMAP.contains("- [x] **Internet P2P threat review**"));
}
