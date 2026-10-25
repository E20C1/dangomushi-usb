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
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");

fn temporary_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-staging-contract-{label}-{}-{unique}",
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

fn create_fixture(label: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let source = temporary_path(&format!("{label}-source"));
    let workspace_root = temporary_path(&format!("{label}-workspace"));
    let snapshot_parent = temporary_path(&format!("{label}-snapshot-parent"));
    let stage_parent = temporary_path(&format!("{label}-stage-parent"));

    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&snapshot_parent).expect("snapshot parent");
    fs::create_dir_all(&stage_parent).expect("stage parent");
    fake_source(&source);

    workspace::initialize(&workspace_root).expect("workspace");
    fs::write(
        workspace_root.join("data/documents/recovery.txt"),
        b"workspace-payload",
    )
    .expect("workspace payload");
    let backup = workspace_backup::create_backup(&workspace_root)
        .expect("backup")
        .relative_path;

    let snapshot = snapshot_parent.join("snapshot");
    recovery_snapshot::create_snapshot(
        &source,
        &workspace_root,
        &backup,
        &snapshot,
    )
    .expect("snapshot");

    (source, workspace_root, snapshot, stage_parent)
}

fn cleanup(paths: &[&Path]) {
    for path in paths {
        let _ = fs::remove_dir_all(path);
    }
}

#[test]
fn verified_snapshot_materializes_exactly_and_reverifies_destination() {
    let (source, workspace_root, snapshot, stage_parent) = create_fixture("success");
    let destination = stage_parent.join("staged");

    let source_summary = recovery_snapshot::verify_snapshot(&snapshot).expect("source verify");
    let staged = recovery_snapshot::stage_verified_snapshot(
        &snapshot,
        &destination,
        Some(&source),
        Some(&workspace_root),
    )
    .expect("stage");

    assert_eq!(staged, source_summary);
    assert!(destination.join(SNAPSHOT_MARKER).is_file());
    assert!(destination.join("Cargo.lock").is_file());
    assert!(destination.join("vendor/crate-1.0.0/src.rs").is_file());
    assert!(destination.join("recovery/workspace.dgmwbk").is_file());
    assert!(!destination.join("workspace").exists());
    assert_eq!(
        recovery_snapshot::verify_snapshot(&destination).expect("staged verify"),
        source_summary
    );

    cleanup(&[
        &source,
        &workspace_root,
        snapshot.parent().expect("snapshot parent"),
        &stage_parent,
    ]);
}

#[test]
fn source_snapshot_must_verify_before_destination_is_created() {
    let (source, workspace_root, snapshot, stage_parent) = create_fixture("tamper");
    fs::write(snapshot.join("src/lib.rs"), b"tampered after snapshot completion")
        .expect("tamper snapshot");
    let destination = stage_parent.join("must-not-exist");

    assert_eq!(
        recovery_snapshot::stage_verified_snapshot(
            &snapshot,
            &destination,
            Some(&source),
            Some(&workspace_root),
        )
        .unwrap_err(),
        RecoverySnapshotError::IntegrityFailed
    );
    assert!(!destination.exists());

    cleanup(&[
        &source,
        &workspace_root,
        snapshot.parent().expect("snapshot parent"),
        &stage_parent,
    ]);
}

#[test]
fn staging_destination_is_create_new_and_outside_all_protected_roots() {
    let (source, workspace_root, snapshot, stage_parent) = create_fixture("boundaries");

    let existing = stage_parent.join("existing");
    fs::create_dir(&existing).expect("existing");
    assert_eq!(
        recovery_snapshot::stage_verified_snapshot(
            &snapshot,
            &existing,
            Some(&source),
            Some(&workspace_root),
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationExists
    );

    assert_eq!(
        recovery_snapshot::stage_verified_snapshot(
            &snapshot,
            &snapshot.join("inside"),
            Some(&source),
            Some(&workspace_root),
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationInsideInput
    );
    assert_eq!(
        recovery_snapshot::stage_verified_snapshot(
            &snapshot,
            &source.join("inside"),
            Some(&source),
            Some(&workspace_root),
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationInsideInput
    );
    assert_eq!(
        recovery_snapshot::stage_verified_snapshot(
            &snapshot,
            &workspace_root.join("inside"),
            Some(&source),
            Some(&workspace_root),
        )
        .unwrap_err(),
        RecoverySnapshotError::DestinationInsideInput
    );

    cleanup(&[
        &source,
        &workspace_root,
        snapshot.parent().expect("snapshot parent"),
        &stage_parent,
    ]);
}

#[test]
fn staging_runtime_reuses_verifier_and_adds_no_rebuild_restore_network_or_secret_authority() {
    let start = CORE
        .find("pub fn stage_verified_snapshot")
        .expect("stage entry point");
    let end = CORE[start..]
        .find("\nfn checked_source_root")
        .map(|offset| start + offset)
        .expect("stage entry end");
    let stage = &CORE[start..end];

    let verify_pos = stage
        .find("verify_snapshot(snapshot_root)?")
        .expect("source verifier");
    let create_pos = stage.find("fs::create_dir").expect("destination creation");
    assert!(
        verify_pos < create_pos,
        "source verifier must complete before destination mutation"
    );

    assert!(stage.contains("copy_regular_file"));
    assert!(stage.contains("verify_snapshot(&destination_path)?"));
    assert!(stage.contains("staged_summary != source_summary"));
    assert!(stage.contains("SNAPSHOT_MARKER"));

    for forbidden in [
        "restore_backup",
        "secret_backup",
        "vault_storage",
        "Command::new",
        "std::process",
        "std::net",
        "TcpStream",
        "TcpListener",
    ] {
        assert!(
            !stage.contains(forbidden),
            "staging gained forbidden authority: {forbidden}"
        );
    }

    assert!(!SESSION.contains("recovery_staging="));
    assert!(!SESSION.contains("staged_recovery_path="));
}

#[test]
fn cli_docs_and_roadmap_advance_exactly_to_frozen_rebuild() {
    assert!(MAIN.contains(r#""stage" =>"#));
    assert!(MAIN.contains(
        "recovery snapshot stage --snapshot <PATH> --destination <PATH>"
    ));
    assert!(MAIN.contains("DANGOMUSHI RECOVERY SNAPSHOT STAGED"));
    assert!(!MAIN.contains(r#""rebuild" =>"#));

    for required in [
        "existing read-only verify_snapshot",
        "before creating the destination",
        "copied last",
        "verified again",
        "does not restore",
        "does not run Cargo",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing staging boundary documentation: {required}"
        );
    }

    assert!(README.contains("verified snapshot staging materialization"));
    assert!(ROADMAP.contains("- [x] **Verified snapshot staging materialization**"));
    assert!(ROADMAP.contains(
        "- [ ] **Frozen rebuild from staged source/vendor** — Exactly One Next Task"
    ));
    assert!(!ROADMAP.contains(
        "- [ ] **Recovered application smoke verification** — Exactly One Next Task"
    ));
}
