use dangomushi::recovery_snapshot::{
    self, RecoverySnapshotError, SNAPSHOT_MARKER,
};
use dangomushi::{workspace, workspace_backup};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CORE: &str = include_str!("../src/recovery_snapshot.rs");
const MAIN: &str = include_str!("../src/main.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/RECOVERY_SNAPSHOTS.md");
const README: &str = include_str!("../README.md");

fn temporary_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-recovery-contract-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn fake_source(root: &Path) {
    fs::create_dir_all(root.join(".cargo")).expect("cargo");
    fs::create_dir_all(root.join(".github/workflows")).expect("workflows");
    for directory in ["docs", "scripts", "src", "tests", "web", "vendor/crate-1.0.0"] {
        fs::create_dir_all(root.join(directory)).expect("directory");
    }

    for (path, bytes) in [
        (".gitignore", b"target/\n".as_slice()),
        (
            ".cargo/vendor-config.toml",
            b"[source.crates-io]\nreplace-with=\"vendored-sources\"\n".as_slice(),
        ),
        ("Cargo.toml", b"[package]\nname=\"fake\"\n".as_slice()),
        ("Cargo.lock", b"# generated\nversion = 4\n".as_slice()),
        (
            "rust-toolchain.toml",
            b"[toolchain]\nchannel=\"1.99.1\"\n".as_slice(),
        ),
        ("README.md", b"# fake\n".as_slice()),
        ("UPDATE_TOOLBOX.cmd", b"@echo off\n".as_slice()),
        (".github/workflows/ci.yml", b"name: CI\n".as_slice()),
        ("docs/ROADMAP.md", b"# roadmap\n".as_slice()),
        ("scripts/update.sh", b"#!/bin/sh\n".as_slice()),
        ("src/lib.rs", b"pub fn fake() {}\n".as_slice()),
        ("tests/contract.rs", b"#[test] fn fake() {}\n".as_slice()),
        ("web/index.html", b"<html></html>\n".as_slice()),
        (
            "vendor/crate-1.0.0/.cargo-checksum.json",
            b"{\"package\":null,\"files\":{}}\n".as_slice(),
        ),
        (
            "vendor/crate-1.0.0/src.rs",
            b"pub fn vendored() {}\n".as_slice(),
        ),
    ] {
        fs::write(root.join(path), bytes).expect("source file");
    }
}

fn workspace_backup_path(root: &Path) -> PathBuf {
    workspace::initialize(root).expect("workspace");
    fs::write(root.join("data/documents/recovery.txt"), b"workspace-payload")
        .expect("workspace payload");
    workspace_backup::create_backup(root)
        .expect("workspace backup")
        .relative_path
}

#[test]
fn recovery_snapshot_is_create_new_and_contains_only_reviewed_recovery_inputs() {
    let source = temporary_path("source");
    let workspace_root = temporary_path("workspace");
    let destination_parent = temporary_path("destination-parent");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&destination_parent).expect("destination parent");
    fake_source(&source);
    let backup = workspace_backup_path(&workspace_root);

    fs::create_dir_all(source.join(".git")).expect("git");
    fs::write(source.join(".git/config"), b"private-git-state").expect("git state");
    fs::create_dir_all(source.join("target")).expect("target");
    fs::write(source.join("target/output"), b"build-output").expect("target output");
    fs::write(source.join(".env"), b"must-not-copy").expect("root secret");

    let destination = destination_parent.join("recovery");
    let summary = recovery_snapshot::create_snapshot(
        &source,
        &workspace_root,
        &backup,
        &destination,
    )
    .expect("create recovery snapshot");

    assert!(summary.files > 0);
    assert!(destination.join(SNAPSHOT_MARKER).is_file());
    assert!(destination.join("Cargo.toml").is_file());
    assert!(destination.join("Cargo.lock").is_file());
    assert!(destination.join("vendor/crate-1.0.0/src.rs").is_file());
    assert!(destination.join("recovery/workspace.dgmwbk").is_file());

    for forbidden in [".git", "target", ".env", "workspace", "app", "build"] {
        assert!(
            !destination.join(forbidden).exists(),
            "unexpected recovery snapshot path: {forbidden}"
        );
    }

    let marker = fs::read_to_string(destination.join(SNAPSHOT_MARKER)).expect("marker");
    assert!(marker.contains("kind=dangomushi-recovery-snapshot"));
    assert!(marker.contains("format=1"));
    assert!(marker.contains("workspace_backup=recovery/workspace.dgmwbk"));
    assert!(marker.contains(&format!("snapshot_sha256={}", summary.sha256_hex())));

    let verified = recovery_snapshot::verify_snapshot(&destination).expect("verify recovery snapshot");
    assert_eq!(verified, summary);

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(workspace_root).expect("cleanup workspace");
    fs::remove_dir_all(destination_parent).expect("cleanup destination");
}

#[test]
fn live_security_and_secret_state_are_not_added_outside_generic_backup_scope() {
    let source = temporary_path("source-security");
    let workspace_root = temporary_path("workspace-security");
    let destination_parent = temporary_path("destination-security");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&destination_parent).expect("destination parent");
    fake_source(&source);
    workspace::initialize(&workspace_root).expect("workspace");

    let peer_secret = b"RECOVERY-SNAPSHOT-MUST-NOT-COPY-PEER-SECRET-41";
    let permission_secret = b"RECOVERY-SNAPSHOT-MUST-NOT-COPY-PERMISSION-42";
    let session_secret = b"RECOVERY-SNAPSHOT-MUST-NOT-COPY-SESSION-43";
    fs::write(workspace_root.join("state/peer_identity.v1"), peer_secret).expect("peer state");
    fs::write(
        workspace_root.join("state/device_permissions.v1.json"),
        permission_secret,
    )
    .expect("permission state");
    fs::write(workspace_root.join("state/session.v1"), session_secret).expect("session state");

    let backup = workspace_backup::create_backup(&workspace_root)
        .expect("backup")
        .relative_path;
    let destination = destination_parent.join("recovery");

    recovery_snapshot::create_snapshot(
        &source,
        &workspace_root,
        &backup,
        &destination,
    )
    .expect("snapshot");

    let copied_backup =
        fs::read(destination.join("recovery/workspace.dgmwbk")).expect("copied backup");
    for forbidden in [peer_secret.as_slice(), permission_secret.as_slice(), session_secret.as_slice()] {
        assert!(
            !copied_backup.windows(forbidden.len()).any(|window| window == forbidden),
            "recovery snapshot must not bypass Generic Workspace Backup exclusions"
        );
    }

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(workspace_root).expect("cleanup workspace");
    fs::remove_dir_all(destination_parent).expect("cleanup destination");
}

#[test]
fn destination_and_vendor_boundaries_fail_closed() {
    let source = temporary_path("source-boundary");
    let workspace_root = temporary_path("workspace-boundary");
    let destination_parent = temporary_path("destination-boundary");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&destination_parent).expect("destination parent");
    fake_source(&source);
    let backup = workspace_backup_path(&workspace_root);

    assert_eq!(
        recovery_snapshot::create_snapshot(
            &source,
            &workspace_root,
            &backup,
            &source.join("snapshot"),
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationInsideInput
    );

    let existing = destination_parent.join("existing");
    fs::create_dir(&existing).expect("existing destination");
    assert_eq!(
        recovery_snapshot::create_snapshot(
            &source,
            &workspace_root,
            &backup,
            &existing,
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationExists
    );

    fs::remove_dir_all(source.join("vendor")).expect("remove vendor");
    assert_eq!(
        recovery_snapshot::create_snapshot(
            &source,
            &workspace_root,
            &backup,
            &destination_parent.join("no-vendor"),
        )
        .unwrap_err(),
        RecoverySnapshotError::VendorUnavailable
    );

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(workspace_root).expect("cleanup workspace");
    fs::remove_dir_all(destination_parent).expect("cleanup destination");
}

#[test]
fn recovery_core_has_no_network_process_or_secret_backup_authority() {
    let production = CORE
        .split("#[cfg(test)]")
        .next()
        .expect("production recovery core");

    for forbidden in [
        "std::net",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "std::process",
        "Command::new",
        "secret_backup",
        "vault_storage",
        "MasterPassphrase",
        "Credential",
        "clipboard",
    ] {
        assert!(
            !production.contains(forbidden),
            "recovery snapshot core gained forbidden authority: {forbidden}"
        );
    }

    assert!(production.contains("workspace_backup::verify_backup"));
    assert!(production.contains("vendor"));
    assert!(production.contains("SNAPSHOT_MARKER"));
    assert!(!SESSION.contains("recovery_snapshot_path="));
    assert!(!SESSION.contains("recovery_snapshot_digest="));
}

#[test]
fn cli_exposes_creation_and_read_only_verification_but_not_restore() {
    assert!(MAIN.contains(r#"Some("recovery") => run_recovery(args.collect())"#));
    assert!(MAIN.contains(r#""create" =>"#));
    assert!(MAIN.contains(r#""verify" =>"#));
    assert!(MAIN.contains("--backup is required for a recovery snapshot"));
    assert!(MAIN.contains("--destination is required for a recovery snapshot"));
    assert!(MAIN.contains("--snapshot is required for recovery snapshot verification"));
    assert!(MAIN.contains("recovery snapshot verify --snapshot <PATH>"));
    assert!(!MAIN.contains(r#""restore" =>"#));
}

#[test]
fn docs_and_roadmap_advance_once_to_restore_verification() {
    for required in [
        "higher-level recovery artifact",
        "does not replace Generic Workspace Backup",
        "Secret Backup remains separate",
        "must be outside both the source root and Workspace root",
        "not publisher authenticity",
        "does not add a restore/extract command",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing recovery snapshot boundary: {required}"
        );
    }

    assert!(README.contains("M5 recovery snapshot"));
    assert!(ROADMAP.contains("- [x] **Recovery snapshots**"));
    assert!(ROADMAP.contains("- [x] **Restore verification**"));
    assert!(ROADMAP.contains(
        "- [ ] **Portable release bundles per operating system** — Exactly One Next Task"
    ));
}
