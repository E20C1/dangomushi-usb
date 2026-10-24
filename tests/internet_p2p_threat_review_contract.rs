use dangomushi::internet_p2p_policy::{
    self, InternetP2PDecision, THREAT_REVIEW_VERSION,
};
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::peer_lan::{self, PeerLanError, DEFAULT_PEER_LAN_PORT};
use dangomushi::peer_transfer::TransferPayload;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::Path;

const POLICY_SOURCE: &str = include_str!("../src/internet_p2p_policy.rs");
const LAN_SOURCE: &str = include_str!("../src/peer_lan.rs");
const SERVER_SOURCE: &str = include_str!("../src/server.rs");
const CARGO: &str = include_str!("../Cargo.toml");
const UI: &str = include_str!("../src/native_ui.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/INTERNET_P2P_THREAT_REVIEW.md");

fn enabled_lan() -> LanEnableControl {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    lan
}

fn send_to(target: SocketAddr) -> PeerLanError {
    peer_lan::send_transfer(
        Path::new("."),
        &enabled_lan(),
        target,
        &[0_u8; 32],
        TransferPayload::Text("must not leave LAN scope".to_string()),
    )
    .unwrap_err()
}

#[test]
fn reviewed_policy_is_explicitly_fail_closed() {
    assert_eq!(THREAT_REVIEW_VERSION, 1);
    assert_eq!(
        internet_p2p_policy::current_decision(),
        InternetP2PDecision::DisabledAfterThreatReview
    );
    assert!(!internet_p2p_policy::internet_p2p_enabled());
    assert_eq!(
        internet_p2p_policy::current_decision().label(),
        "DISABLED / THREAT REVIEW COMPLETE"
    );

    let production = POLICY_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production policy");
    for forbidden in [
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "std::net",
        "std::env",
        "Command::new",
        "serde_json",
    ] {
        assert!(
            !production.contains(forbidden),
            "policy must not gain runtime/network override authority: {forbidden}"
        );
    }
}

#[test]
fn direct_lan_still_rejects_every_internet_address_class_before_connect() {
    let cases = [
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(8, 8, 8, 8),
            DEFAULT_PEER_LAN_PORT,
        )),
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(100, 64, 0, 1),
            DEFAULT_PEER_LAN_PORT,
        )),
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(203, 0, 113, 10),
            DEFAULT_PEER_LAN_PORT,
        )),
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::LOCALHOST,
            DEFAULT_PEER_LAN_PORT,
            0,
            0,
        )),
        SocketAddr::V6(SocketAddrV6::new(
            "2001:db8::1".parse().expect("IPv6"),
            DEFAULT_PEER_LAN_PORT,
            0,
            0,
        )),
    ];

    for target in cases {
        assert_eq!(
            send_to(target),
            PeerLanError::UnsupportedAddress,
            "target must remain outside Direct LAN scope: {target}"
        );
    }
}

#[test]
fn no_public_discovery_relay_nat_or_firewall_surface_is_added() {
    let production = LAN_SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production LAN source");

    for forbidden in [
        "UdpSocket",
        "Ipv6Addr::UNSPECIFIED",
        "multicast",
        "broadcast",
        "rendezvous",
        "hole_punch",
        "hole-punch",
        "STUN",
        "TURN",
        "UPnP",
        "NAT-PMP",
        "PortMapping",
        "Command::new",
    ] {
        assert!(
            !production.contains(forbidden),
            "Direct LAN must not silently grow an Internet surface: {forbidden}"
        );
    }

    assert!(SERVER_SOURCE.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER_SOURCE.contains("0.0.0.0"));
    assert!(!SERVER_SOURCE.contains("internet_p2p_policy::"));

    assert!(!CARGO.contains("internet-p2p"));
    assert!(!CARGO.contains("internet_p2p"));
    assert!(!CARGO.contains("stun"));
    assert!(!CARGO.contains("turn"));
    assert!(!CARGO.contains("natpmp"));
    assert!(!CARGO.contains("igd ="));
}

#[test]
fn threat_review_covers_new_internet_authorities_and_sets_reconsideration_gates() {
    for required in [
        "Public listener and unsolicited reachability",
        "Global address support",
        "NAT traversal and hole punching",
        "Automatic router/firewall mutation",
        "Rendezvous/discovery service",
        "Relay service",
        "Denial of service and resource exhaustion",
        "Malicious trusted peer",
        "Identity-key compromise",
        "Metadata and traffic analysis",
        "Replay, downgrade and protocol evolution",
        "Availability and local-first dependency",
        "Preconditions before Internet P2P can be reconsidered",
    ] {
        assert!(DOC.contains(required), "missing threat-review section: {required}");
    }

    for required in [
        "no runtime/configuration override",
        "global/CGNAT/IPv6 targets",
        "M4 therefore closes",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing fail-closed decision evidence: {required}"
        );
    }
}

#[test]
fn ui_and_roadmap_make_disabled_decision_unambiguous_and_advance_to_m5() {
    assert!(POLICY_SOURCE.contains("DISABLED / THREAT REVIEW COMPLETE"));
    assert!(UI.contains("internet_p2p_policy::current_decision().label()"));
    assert!(UI.contains("Internet P2P threat review is complete"));
    assert!(!UI.contains("ENABLE INTERNET P2P"));
    assert!(!UI.contains("ENABLE P2P"));

    assert!(ROADMAP.contains("- [x] **Internet P2P threat review**"));
    assert!(ROADMAP.contains(
        "- [ ] **Offline documentation packs** — Exactly One Next Task"
    ));
    assert!(!ROADMAP.contains(
        "- [ ] **Internet P2P only after a separate threat review** — Exactly One Next Task"
    ));
}
