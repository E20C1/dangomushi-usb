use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing, PairingSubmission,
    PairingStatus, MAX_PAIRING_ATTEMPTS, PAIRING_LIFETIME,
};
use dangomushi::lan_access_control::LanEnableControl;
use ed25519_dalek::{Signer, SigningKey};
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!("../src/device_pairing.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const SERVER: &str = include_str!("../src/server.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const CARGO: &str = include_str!("../Cargo.toml");

#[test]
fn pairing_core_uses_existing_crypto_crates_and_has_no_persistence_or_listener_surface() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        "getrandom::fill",
        "Hmac<Sha256>",
        "verify_slice",
        "VerifyingKey::from_bytes",
        "verify_strict",
        "Zeroizing",
        "MAX_PAIRING_ATTEMPTS",
        "PAIRING_LIFETIME",
        "LanEnableControl",
        "AwaitingApproval",
    ] {
        assert!(
            production.contains(required),
            "missing device-pairing boundary evidence: {required}"
        );
    }

    for forbidden in [
        "std::fs",
        "OpenOptions",
        "File::",
        "workspace_backup",
        "session_state",
        "TcpListener",
        "TcpStream",
        "std::net",
        "Command::new",
    ] {
        assert!(
            !production.contains(forbidden),
            "pairing core must remain process-local and transport-free: {forbidden}"
        );
    }

    assert!(CARGO.contains("hmac = \"0.13.0\""));
    assert!(CARGO.contains("ed25519-dalek = \"3.0.0\""));
}

#[test]
fn end_to_end_core_requires_code_proof_private_key_possession_and_owner_approval() {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();

    let mut pairing = DevicePairing::default();
    let invitation = pairing.begin(&lan, now).expect("begin pairing");

    let signing_key = SigningKey::from_bytes(&[21u8; 32]);
    let public_key = signing_key.verifying_key().to_bytes();
    let transcript =
        pairing_transcript(invitation.challenge(), "Pixel", &public_key).expect("transcript");
    let proof = pairing_proof(
        invitation.code().expose_secret(),
        invitation.challenge(),
        "Pixel",
        &public_key,
    )
    .expect("proof");
    let signature = signing_key
        .sign(&pairing_signature_message(&transcript, &proof))
        .to_bytes();

    let summary = pairing
        .submit(
            &lan,
            now + Duration::from_secs(1),
            PairingSubmission {
                device_name: "Pixel".to_string(),
                public_key,
                code_proof: proof,
                signature,
            },
        )
        .expect("cryptographic proof");
    assert_eq!(summary.device_name(), "Pixel");
    assert!(matches!(
        pairing.status(now + Duration::from_secs(1)),
        PairingStatus::AwaitingApproval { .. }
    ));

    let paired = pairing
        .approve(&lan, now + Duration::from_secs(2))
        .expect("explicit approval");
    assert_eq!(paired.public_key(), &public_key);
}

#[test]
fn native_pairing_surface_is_explicit_run_local_and_transport_locked() {
    for required in [
        "START DEVICE PAIRING",
        "PAIRING CODE",
        "PAIRING CORE READY / TRANSPORT LOCKED",
        "APPROVE DEVICE FOR THIS RUN",
        "PAIRED FOR THIS RUN",
        "TRUST DEVICE PERSISTENTLY",
        "self.device_pairing.cancel()",
    ] {
        assert!(UI.contains(required), "missing native pairing UI evidence: {required}");
    }

    for forbidden in [
        "pairing_code=",
        "pairing_challenge=",
        "paired_device=",
        "trusted_device=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist pairing state: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(SERVER.contains("is_loopback()"));
    assert!(!SERVER.contains("0.0.0.0"));
}

#[test]
fn bounded_pairing_contract_advances_only_to_persistent_trusted_keys() {
    assert_eq!(MAX_PAIRING_ATTEMPTS, 5);
    assert_eq!(PAIRING_LIFETIME, Duration::from_secs(300));

    for required in [
        "80 bits of entropy",
        "five minutes",
        "at most five failed cryptographic proofs",
        "strict verification",
        "does not itself create durable trust",
        "No non-loopback listener or pairing HTTP route is added",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing architecture pairing clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Explicit LAN enable control**"));
    assert!(ROADMAP.contains("- [x] **Device pairing**"));
    assert!(ROADMAP.contains("- [x] **Mobile editor/session handoff**"));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
