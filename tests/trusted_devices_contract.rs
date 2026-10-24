use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing, PairingSubmission,
};
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::trusted_devices::{self, TrustedDeviceError, MAX_TRUSTED_DEVICES};
use dangomushi::workspace;
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/trusted_devices.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const SERVER: &str = include_str!("../src/server.rs");
const BACKUP: &str = include_str!("../src/workspace_backup.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");

fn temporary_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-trusted-device-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn approved_candidate() -> dangomushi::device_pairing::PairedDeviceCandidate {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();

    let mut pairing = DevicePairing::default();
    let invitation = pairing.begin(&lan, now).expect("begin pairing");
    let signing_key = SigningKey::from_bytes(&[41u8; 32]);
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

    pairing
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
        .expect("submit");
    pairing
        .approve(&lan, now + Duration::from_secs(2))
        .expect("approve")
}

#[test]
fn approved_public_key_persists_across_process_local_pairing_lifetime() {
    let root = temporary_workspace("persist");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate();
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");

    assert_eq!(trusted.device_name(), "Pixel");
    assert_eq!(trusted.public_key(), candidate.public_key());

    let loaded = trusted_devices::load(&root).expect("reload trust store");
    assert_eq!(loaded.devices().len(), 1);
    assert_eq!(loaded.devices()[0].public_key(), candidate.public_key());
    assert!(root.join("identity/trusted_devices.v1.json").is_file());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn duplicate_candidate_is_rejected_without_creating_duplicate_trust() {
    let root = temporary_workspace("duplicate");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate();
    trusted_devices::register_candidate(&root, &candidate).expect("first trust");
    assert_eq!(
        trusted_devices::register_candidate(&root, &candidate),
        Err(TrustedDeviceError::AlreadyTrusted)
    );

    let loaded = trusted_devices::load(&root).expect("load");
    assert_eq!(loaded.devices().len(), 1);

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn persistent_trust_core_is_bounded_verified_and_identity_scoped() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    assert_eq!(MAX_TRUSTED_DEVICES, 256);

    for required in [
        r#""identity""#,
        r#""trusted_devices.v1.json""#,
        r#""trusted_devices.v1.pending""#,
        r#""trusted_devices.v1.previous""#,
        "fs::symlink_metadata",
        "create_new(true)",
        "sync_all",
        "decode_store",
        "device_key_fingerprint",
        "AlreadyTrusted",
        "MAX_TRUSTED_DEVICES",
        "MAX_TRUST_STORE_BYTES",
    ] {
        assert!(
            production.contains(required),
            "missing trusted-device storage boundary evidence: {required}"
        );
    }

    for forbidden in [
        "PairingCode",
        "code_proof",
        "signature:",
        "private_key",
        "TcpListener",
        "TcpStream",
        "std::net",
        "Command::new",
        "pub fn revoke",
        "pub fn remove",
    ] {
        assert!(
            !production.contains(forbidden),
            "persistent public-key store must not expand scope: {forbidden}"
        );
    }

    assert!(BACKUP.contains(r#"&["identity"]"#));
}

#[test]
fn native_ui_keeps_persistent_trust_explicit_and_hands_revocation_to_its_own_boundary() {
    for required in [
        "PERSISTENT TRUSTED DEVICES",
        "TRUST DEVICE PERSISTENTLY",
        "REFRESH TRUST STORE",
        "workspace:/identity",
        "REVOKE DEVICE",
        "trusted_devices::register_candidate",
        "device_revocation::revoke",
    ] {
        assert!(
            UI.contains(required),
            "missing native trusted-device UI evidence: {required}"
        );
    }

    for forbidden in [
        "trusted_device=",
        "trusted_public_key=",
        "trusted_fingerprint=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not duplicate durable trust material: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
}

#[test]
fn architecture_keeps_trust_store_separate_and_advances_to_permission_scopes() {
    for required in [
        "workspace/identity/trusted_devices.v1.json",
        "bounded to 256 devices and 256 KiB",
        "never promotes a pending-only first write",
        "Device revocation below adds a monotonic reconciliation rule",
        "trusted_devices::load is an effective-trust read",
        "same public key cannot be trusted again after revocation",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing trusted-device architecture clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Persistent trusted-device public keys**"));
    assert!(ROADMAP.contains("- [x] **Device revocation**"));
    assert!(ROADMAP.contains(
        "- [ ] **Permission scopes** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
