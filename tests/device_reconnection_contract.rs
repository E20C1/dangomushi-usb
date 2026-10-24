use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing,
    PairedDeviceCandidate, PairingSubmission,
};
use dangomushi::device_permissions::{self, PermissionScope};
use dangomushi::device_reconnection::{
    self, ReconnectError, ReconnectProof, ReconnectRandomSource, ReconnectRandomSourceError,
    RECONNECT_CHALLENGE_BYTES, RECONNECT_CHALLENGE_LIFETIME,
};
use dangomushi::device_revocation;
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::trusted_devices;
use dangomushi::workspace;
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/device_reconnection.rs");
const SESSION_STATE: &str = include_str!("../src/session_state.rs");
const SERVER: &str = include_str!("../src/server.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const UI: &str = include_str!("../src/native_ui.rs");

struct FixedRandom([u8; RECONNECT_CHALLENGE_BYTES]);

impl ReconnectRandomSource for FixedRandom {
    fn fill_bytes(
        &mut self,
        destination: &mut [u8],
    ) -> Result<(), ReconnectRandomSourceError> {
        destination.copy_from_slice(&self.0);
        Ok(())
    }
}

fn temporary_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-device-reconnect-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn approved_candidate(seed: u8, name: &str) -> (PairedDeviceCandidate, SigningKey) {
    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();

    let mut pairing = DevicePairing::default();
    let invitation = pairing.begin(&lan, now).expect("begin pairing");
    let signing_key = SigningKey::from_bytes(&[seed; 32]);
    let public_key = signing_key.verifying_key().to_bytes();

    let transcript =
        pairing_transcript(invitation.challenge(), name, &public_key).expect("transcript");
    let proof = pairing_proof(
        invitation.code().expose_secret(),
        invitation.challenge(),
        name,
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
                device_name: name.to_string(),
                public_key,
                code_proof: proof,
                signature,
            },
        )
        .expect("submit");
    let candidate = pairing
        .approve(&lan, now + Duration::from_secs(2))
        .expect("approve");
    (candidate, signing_key)
}

fn reconnect_session(
    root: &std::path::Path,
    lan: &LanEnableControl,
    fingerprint: &[u8; 32],
    signing_key: &SigningKey,
    now: Instant,
) -> dangomushi::device_reconnection::AuthenticatedDeviceSession {
    let mut random = FixedRandom([73u8; RECONNECT_CHALLENGE_BYTES]);
    let challenge =
        device_reconnection::begin_with_source(root, lan, fingerprint, now, &mut random)
            .expect("challenge");
    let signature = signing_key
        .sign(&device_reconnection::reconnect_signature_message(
            challenge.challenge(),
            challenge.fingerprint(),
        ))
        .to_bytes();
    device_reconnection::complete(
        root,
        lan,
        now + Duration::from_secs(1),
        challenge,
        ReconnectProof {
            fingerprint: *fingerprint,
            signature,
        },
    )
    .expect("complete reconnect")
}

#[test]
fn trusted_device_reconnects_with_fresh_key_proof_and_current_permissions() {
    let root = temporary_workspace("success");
    workspace::initialize(&root).expect("workspace");

    let (candidate, signing_key) = approved_candidate(81, "Phone");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trust");
    device_permissions::set_scopes(
        &root,
        trusted.fingerprint(),
        &[PermissionScope::StatusRead],
    )
    .expect("grant status");

    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();
    let session = reconnect_session(&root, &lan, trusted.fingerprint(), &signing_key, now);

    assert_eq!(session.fingerprint(), trusted.fingerprint());
    assert_eq!(session.device_name(), "Phone");
    assert!(session
        .authorize(&root, &lan, PermissionScope::StatusRead)
        .expect("status auth"));
    assert!(!session
        .authorize(&root, &lan, PermissionScope::FilesRead)
        .expect("files auth"));

    device_permissions::set_scopes(&root, trusted.fingerprint(), &[])
        .expect("remove permissions");
    assert!(!session
        .authorize(&root, &lan, PermissionScope::StatusRead)
        .expect("live permission removal"));

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn revocation_or_lan_disable_invalidates_an_already_authenticated_principal() {
    let root = temporary_workspace("invalidate");
    workspace::initialize(&root).expect("workspace");

    let (candidate, signing_key) = approved_candidate(82, "Tablet");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trust");
    device_permissions::set_scopes(
        &root,
        trusted.fingerprint(),
        &[PermissionScope::FilesRead],
    )
    .expect("grant");

    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let session = reconnect_session(
        &root,
        &lan,
        trusted.fingerprint(),
        &signing_key,
        Instant::now(),
    );
    assert!(session.is_currently_valid(&root, &lan).expect("valid"));

    device_revocation::revoke(&root, trusted.fingerprint()).expect("revoke");
    assert!(!session.is_currently_valid(&root, &lan).expect("revoked invalid"));
    assert!(!session
        .authorize(&root, &lan, PermissionScope::FilesRead)
        .expect("revoked denied"));

    lan.disable();
    assert!(!session.is_currently_valid(&root, &lan).expect("LAN disabled"));

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn stale_or_replayed_proof_is_rejected() {
    let root = temporary_workspace("proof");
    workspace::initialize(&root).expect("workspace");

    let (candidate, signing_key) = approved_candidate(83, "Laptop");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trust");

    let mut lan = LanEnableControl::default();
    lan.enable_for_this_run();
    let now = Instant::now();

    let mut first_random = FixedRandom([10u8; RECONNECT_CHALLENGE_BYTES]);
    let first = device_reconnection::begin_with_source(
        &root,
        &lan,
        trusted.fingerprint(),
        now,
        &mut first_random,
    )
    .expect("first challenge");
    let first_signature = signing_key
        .sign(&device_reconnection::reconnect_signature_message(
            first.challenge(),
            first.fingerprint(),
        ))
        .to_bytes();

    let mut second_random = FixedRandom([11u8; RECONNECT_CHALLENGE_BYTES]);
    let second = device_reconnection::begin_with_source(
        &root,
        &lan,
        trusted.fingerprint(),
        now,
        &mut second_random,
    )
    .expect("second challenge");
    assert_eq!(
        device_reconnection::complete(
            &root,
            &lan,
            now + Duration::from_secs(1),
            second,
            ReconnectProof {
                fingerprint: *trusted.fingerprint(),
                signature: first_signature,
            },
        ),
        Err(ReconnectError::InvalidSignature)
    );

    let mut expired_random = FixedRandom([12u8; RECONNECT_CHALLENGE_BYTES]);
    let expired = device_reconnection::begin_with_source(
        &root,
        &lan,
        trusted.fingerprint(),
        now,
        &mut expired_random,
    )
    .expect("expired challenge");
    let expired_signature = signing_key
        .sign(&device_reconnection::reconnect_signature_message(
            expired.challenge(),
            expired.fingerprint(),
        ))
        .to_bytes();
    assert_eq!(
        device_reconnection::complete(
            &root,
            &lan,
            now + RECONNECT_CHALLENGE_LIFETIME,
            expired,
            ReconnectProof {
                fingerprint: *trusted.fingerprint(),
                signature: expired_signature,
            },
        ),
        Err(ReconnectError::Expired)
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn authenticated_session_has_no_fixed_expiry_or_durable_bearer_secret() {
    let session_decl = SOURCE
        .split("pub struct AuthenticatedDeviceSession")
        .nth(1)
        .expect("session declaration")
        .split("impl AuthenticatedDeviceSession")
        .next()
        .expect("session fields");

    for forbidden in [
        "expires_at",
        "token",
        "secret",
        "cookie",
        "refresh",
        "Instant",
        "Duration",
    ] {
        assert!(
            !session_decl.contains(forbidden),
            "authenticated principal must not contain fixed-expiry/bearer state: {forbidden}"
        );
    }

    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    for forbidden in [
        "std::fs",
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
            "reconnect core must remain ephemeral and transport-free: {forbidden}"
        );
    }

    for forbidden in [
        "reconnect_token=",
        "refresh_token=",
        "authenticated_device=",
        "reconnect_session=",
    ] {
        assert!(
            !SESSION_STATE.contains(forbidden),
            "generic session must not persist reconnect credentials: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
}

#[test]
fn architecture_and_roadmap_advance_exactly_to_public_network_safe_default() {
    for required in [
        "fresh 256-bit challenge",
        "no fixed expiry field",
        "Scopes are not snapshotted",
        "does not require a new pairing code",
        "does not add a LAN listener",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing reconnect architecture clause: {required}"
        );
    }

    for required in [
        "FRESH KEY PROOF / NO FIXED TTL",
        "fresh Ed25519 challenge",
        "No persistent bearer token",
    ] {
        assert!(UI.contains(required), "missing reconnect UI evidence: {required}");
    }

    assert!(ROADMAP.contains(
        "- [x] **Reconnection without short fixed session expiry**"
    ));
    assert!(ROADMAP.contains(
        "- [ ] **Public-network safe default** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
