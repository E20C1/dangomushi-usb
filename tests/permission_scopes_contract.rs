use dangomushi::device_pairing::{
    pairing_proof, pairing_signature_message, pairing_transcript, DevicePairing,
    PairedDeviceCandidate, PairingSubmission,
};
use dangomushi::device_permissions::{
    self, DevicePermissionError, PermissionScope, MAX_PERMISSION_DEVICES,
};
use dangomushi::device_revocation;
use dangomushi::lan_access_control::LanEnableControl;
use dangomushi::trusted_devices;
use dangomushi::{workspace, workspace_backup};
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("../src/device_permissions.rs");
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
        "dangomushi-device-permissions-{label}-{}-{unique}",
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
fn trusted_device_starts_default_deny_and_explicit_scopes_authorize() {
    let root = temporary_workspace("default-deny");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate(61, "Pixel");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");

    for scope in PermissionScope::ALL {
        assert!(!device_permissions::authorize(&root, trusted.fingerprint(), scope)
            .expect("default authorization"));
    }

    let policy = device_permissions::set_scopes(
        &root,
        trusted.fingerprint(),
        &[PermissionScope::FilesRead, PermissionScope::StatusRead],
    )
    .expect("set scopes");
    assert_eq!(
        policy.scopes(),
        &[PermissionScope::StatusRead, PermissionScope::FilesRead]
    );

    assert!(device_permissions::authorize(
        &root,
        trusted.fingerprint(),
        PermissionScope::StatusRead
    )
    .expect("status authorization"));
    assert!(device_permissions::authorize(
        &root,
        trusted.fingerprint(),
        PermissionScope::FilesRead
    )
    .expect("read authorization"));
    assert!(!device_permissions::authorize(
        &root,
        trusted.fingerprint(),
        PermissionScope::FilesWrite
    )
    .expect("write authorization"));

    assert!(root.join("state/device_permissions.v1.json").is_file());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn revocation_blocks_authorization_even_if_permission_record_remains() {
    let root = temporary_workspace("revoked");
    workspace::initialize(&root).expect("workspace");

    let candidate = approved_candidate(62, "Tablet");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");
    device_permissions::set_scopes(
        &root,
        trusted.fingerprint(),
        &[PermissionScope::FilesRead, PermissionScope::FilesWrite],
    )
    .expect("grant scopes");

    device_revocation::revoke(&root, trusted.fingerprint()).expect("revoke");

    assert!(!device_permissions::authorize(
        &root,
        trusted.fingerprint(),
        PermissionScope::FilesRead
    )
    .expect("revoked authorization"));
    assert_eq!(
        device_permissions::set_scopes(
            &root,
            trusted.fingerprint(),
            &[PermissionScope::StatusRead],
        ),
        Err(DevicePermissionError::DeviceNotTrusted)
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn generic_restore_does_not_roll_back_permissions_and_retrust_resets_stale_policy() {
    let root = temporary_workspace("restore");
    workspace::initialize(&root).expect("workspace");

    let before_trust = workspace_backup::create_backup(&root).expect("pre-trust backup");

    let candidate = approved_candidate(63, "Phone");
    let trusted =
        trusted_devices::register_candidate(&root, &candidate).expect("persist trusted key");
    device_permissions::set_scopes(
        &root,
        trusted.fingerprint(),
        &[PermissionScope::FilesRead, PermissionScope::SessionHandoff],
    )
    .expect("grant scopes");

    workspace_backup::restore_backup(&root, &before_trust.relative_path).expect("restore old backup");

    let permission_store = device_permissions::load(&root).expect("permissions survive restore");
    assert!(permission_store.allows(trusted.fingerprint(), PermissionScope::FilesRead));
    assert!(trusted_devices::load(&root).expect("trust after restore").devices().is_empty());

    let retrusted =
        trusted_devices::register_candidate(&root, &candidate).expect("explicit re-trust");
    assert_eq!(retrusted.fingerprint(), trusted.fingerprint());
    for scope in PermissionScope::ALL {
        assert!(!device_permissions::authorize(&root, retrusted.fingerprint(), scope)
            .expect("re-trust must reset stale permissions"));
    }

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn generic_backup_does_not_import_permission_grants_to_another_workspace() {
    let source = temporary_workspace("portable-source");
    let destination = temporary_workspace("portable-destination");
    workspace::initialize(&source).expect("source");
    workspace::initialize(&destination).expect("destination");

    let candidate = approved_candidate(64, "Laptop");
    let trusted =
        trusted_devices::register_candidate(&source, &candidate).expect("persist trusted key");
    device_permissions::set_scopes(
        &source,
        trusted.fingerprint(),
        &[PermissionScope::StatusRead, PermissionScope::FilesRead],
    )
    .expect("grant scopes");

    let backup = workspace_backup::create_backup(&source).expect("backup");
    copy_backup(&source, &destination, &backup.relative_path);
    workspace_backup::restore_backup(&destination, &backup.relative_path).expect("restore");

    let restored_trust = trusted_devices::load(&destination).expect("restored trust");
    assert_eq!(restored_trust.devices().len(), 1);
    assert!(!destination.join("state/device_permissions.v1.json").exists());
    assert!(!device_permissions::authorize(
        &destination,
        trusted.fingerprint(),
        PermissionScope::StatusRead
    )
    .expect("destination default deny"));

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(destination).expect("cleanup destination");
}

#[test]
fn permission_core_is_bounded_fixed_scope_and_network_free() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    assert_eq!(MAX_PERMISSION_DEVICES, 256);

    for required in [
        r#""device_permissions.v1.json""#,
        r#""device_permissions.v1.pending""#,
        r#""device_permissions.v1.previous""#,
        r#""status.read""#,
        r#""files.read""#,
        r#""files.write""#,
        r#""session.handoff""#,
        "create_new(true)",
        "sync_all",
        "DeviceNotTrusted",
        "TrustedStateUnavailable",
        "reset_for_new_trust",
        "MAX_PERMISSION_STORE_BYTES",
    ] {
        assert!(
            production.contains(required),
            "missing permission boundary evidence: {required}"
        );
    }

    for forbidden in [
        "TcpListener",
        "TcpStream",
        "std::net",
        "Command::new",
        "0.0.0.0",
        "Vault",
        "RecoveryCode",
    ] {
        assert!(
            !production.contains(forbidden),
            "permission task must not expand scope: {forbidden}"
        );
    }

    assert!(TRUSTED.contains("PermissionStateUnavailable"));
    assert!(TRUSTED.contains("device_permissions::reset_for_new_trust"));
    assert!(BACKUP.contains(r#"&["state", "autosave"]"#));
    assert!(!BACKUP.contains(r#"&["state"]"#));
}

#[test]
fn native_ui_edits_scopes_explicitly_and_session_does_not_persist_drafts() {
    for required in [
        "DEVICE PERMISSIONS",
        "EDIT PERMISSIONS",
        "SAVE PERMISSIONS",
        "DEFAULT DENY",
        "device_permissions::set_scopes",
        "PermissionScope::ALL",
    ] {
        assert!(UI.contains(required), "missing permission UI evidence: {required}");
    }

    for forbidden in [
        "permission_editor_target=",
        "permission_editor_scopes=",
        "device_permission=",
        "permission_scope=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "generic session must not persist permission editor state: {forbidden}"
        );
    }

    assert!(SERVER.contains(r#"TcpListener::bind(("127.0.0.1", port))"#));
    assert!(!SERVER.contains("0.0.0.0"));
}

#[test]
fn architecture_and_roadmap_advance_exactly_to_public_network_safety() {
    for required in [
        "workspace/state/device_permissions.v1.json",
        "default deny",
        "status.read",
        "files.read",
        "files.write",
        "session.handoff",
        "Generic Workspace Backup intentionally excludes",
        "newly trusted key starts without stale authorization",
        "Scopes are not snapshotted",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "missing permission architecture clause: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Permission scopes**"));
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
