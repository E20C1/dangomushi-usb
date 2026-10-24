use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing,
    PairedDeviceCandidate, PairingSubmission,
};
use dangomushi::device_revocation::{self, DeviceRevocationError};
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::trusted_devices::{self, TrustedDeviceError};
use dangomushi::{workspace, workspace_backup};
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/device_revocation.rs");
const TRUSTED: &str = include_str!("../src/trusted_devices.rs");
const BACKUP: &str = include_str!("../src/workspace_backup.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const SERVER: &str = include_str!("../src/server.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");

fn temporary_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-device-revocation-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn approved_candidate(seed: u8, name: &str) -> PairedDeviceCandidate {
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
    pairing
        .approve(&lan, now + Duration::from_secs(2))
        .expect("approve")
}

fn copy_backup(source_root: &Path, destination_root: &Path, relative_backup: &Path) {
    let name = relative_backup.file_name().expect("backup file name");
    fs::copy(
        source_root.join(relative_backup),
        destination_root.join("backups").join(name),
    )
    .expect("copy backup");
}

#[test]
fn revoked_key_is_filtered_and_cannot_be_registered_again() {
    let root = temporary_workspace("effective");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate(31, "Pixel");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");
    assert_eq!(trusted_devices::load(&root).expect("trusted").devices().len(), 1);

    let revoked = device_revocation::revoke(&root, trusted.fingerprint()).expect("revoke");
    assert_eq!(revoked.device_name(), "Pixel");

    let ledger = device_revocation::load(&root).expect("ledger");
    assert!(ledger.contains(trusted.fingerprint()));
    assert!(trusted_devices::load(&root).expect("effective trust").devices().is_empty());
    assert_eq!(
        trusted_devices::register_candidate(&root, &candidate),
        Err(TrustedDeviceError::RevokedDevice)
    );
    assert_eq!(
        device_revocation::revoke(&root, trusted.fingerprint()),
        Err(DeviceRevocationError::AlreadyRevoked)
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn restoring_older_backup_cannot_resurrect_locally_revoked_key() {
    let root = temporary_workspace("restore-old");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate(41, "Tablet");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");
    let old_backup = workspace_backup::create_backup(&root).expect("old backup");

    device_revocation::revoke(&root, trusted.fingerprint()).expect("revoke");
    assert!(trusted_devices::load(&root).expect("effective trust").devices().is_empty());

    workspace_backup::restore_backup(&root, &old_backup.relative_path).expect("restore old backup");

    let ledger = device_revocation::load(&root).expect("ledger after restore");
    assert!(ledger.contains(trusted.fingerprint()));
    assert!(trusted_devices::load(&root).expect("trust after restore").devices().is_empty());
    assert_eq!(
        trusted_devices::register_candidate(&root, &candidate),
        Err(TrustedDeviceError::RevokedDevice)
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn post_revocation_backup_carries_tombstone_to_another_workspace() {
    let source = temporary_workspace("portable-source");
    let destination = temporary_workspace("portable-destination");
    workspace::initialize(&source).expect("source workspace");
    workspace::initialize(&destination).expect("destination workspace");

    let candidate = approved_candidate(51, "Phone");
    let trusted =
        trusted_devices::register_candidate(&source, &candidate).expect("persist trusted key");
    device_revocation::revoke(&source, trusted.fingerprint()).expect("revoke");
    let backup = workspace_backup::create_backup(&source).expect("backup after revocation");

    copy_backup(&source, &destination, &backup.relative_path);
    workspace_backup::restore_backup(&destination, &backup.relative_path).expect("portable restore");

    let ledger = device_revocation::load(&destination).expect("portable ledger");
    assert!(ledger.contains(trusted.fingerprint()));
    assert!(trusted_devices::load(&destination)
        .expect("portable effective trust")
        .devices()
        .is_empty());

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(destination).expect("cleanup destination");
}

#[test]
fn revocation_core_is_bounded_and_restore_is_monotonic() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        r#""device_revocations.v1.json""#,
        r#""device_revocations.v1.pending""#,
        r#""device_revocations.v1.previous""#,
        "MAX_REVOKED_DEVICE_KEYS",
        "MAX_REVOCATION_STORE_BYTES",
        "BTreeSet",
        "create_new(true)",
        "sync_all",
        "merge_for_workspace_restore",
        "AlreadyRevoked",
        "DeviceNotTrusted",
    ] {
        assert!(
            production.contains(required),
            "missing revocation boundary evidence: {required}"
        );
    }

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "std::net",
        "Command::new",
        "pub fn unrevoke",
        "pub fn clear_revocations",
    ] {
        assert!(
            !production.contains(forbidden),
            "revocation task must not expand scope: {forbidden}"
        );
    }

    assert!(TRUSTED.contains("RevokedDevice"));
    assert!(TRUSTED.contains("RevocationStateUnavailable"));
    assert!(TRUSTED.contains("revocations.contains"));
    assert!(BACKUP.contains("device_revocation::merge_for_workspace_restore"));
    assert!(BACKUP.contains("RevocationStateUnavailable"));
}

#[test]
fn native_ui_requires_explicit_revoke_confirmation_and_listener_stays_locked() {
    for required in [
        "REVOKE DEVICE",
        "TYPE REVOKE TO CONFIRM",
        r#"revocation_confirmation.trim() == "REVOKE""#,
        "device_revocation::revoke",
        "key rotation",
    ] {
        assert!(UI.contains(required), "missing revocation UI evidence: {required}");
    }

    for forbidden in [
        "revocation_confirmation=",
        "pending_revocation=",
        "revoked_fingerprint=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist revocation UI state: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
}

#[test]
fn architecture_and_roadmap_advance_exactly_to_permission_scopes() {
    for required in [
        "workspace/identity/device_revocations.v1.json",
        "set union",
        "cannot remove a fingerprint that has already been revoked locally",
        "fails closed",
        "same public key cannot be trusted again",
        "Permission scopes are defined separately below",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing revocation architecture clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Device revocation**"));
    assert!(ROADMAP.contains(
        "- [ ] **Mobile editor/session handoff** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains(
        "No LAN listener should be merged before these security boundaries are implemented and tested."
    ));
}
