use dangomushi::recovery_snapshot::{self, RecoverySnapshotError, SNAPSHOT_MARKER};
use dangomushi::{workspace, workspace_backup};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CORE: &str = include_str!("../src/recovery_snapshot.rs");
const MAIN: &str = include_str!("../src/main.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/RESTORE_VERIFICATION.md");
const README: &str = include_str!("../README.md");

fn temporary_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-restore-verification-{label}-{}-{unique}",
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

struct Fixture {
    source: PathBuf,
    workspace_root: PathBuf,
    parent: PathBuf,
    snapshot: PathBuf,
    created: recovery_snapshot::RecoverySnapshotSummary,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let source = temporary_path(&format!("{label}-source"));
        let workspace_root = temporary_path(&format!("{label}-workspace"));
        let parent = temporary_path(&format!("{label}-parent"));
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&parent).expect("parent");
        fake_source(&source);

        workspace::initialize(&workspace_root).expect("workspace");
        fs::write(
            workspace_root.join("data/documents/recovery.txt"),
            b"restore-verification-payload",
        )
        .expect("workspace payload");
        let backup = workspace_backup::create_backup(&workspace_root)
            .expect("backup")
            .relative_path;

        let snapshot = parent.join("snapshot");
        let created =
            recovery_snapshot::create_snapshot(&source, &workspace_root, &backup, &snapshot)
                .expect("snapshot");

        Self {
            source,
            workspace_root,
            parent,
            snapshot,
            created,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.source);
        let _ = fs::remove_dir_all(&self.workspace_root);
        let _ = fs::remove_dir_all(&self.parent);
    }
}

#[derive(Clone)]
struct TestRecord {
    path: String,
    bytes: u64,
    sha256: [u8; 32],
}

fn rewrite_marker_for_current_payload(snapshot: &Path) {
    let mut records = Vec::new();
    collect_records(snapshot, snapshot, &mut records);
    records.sort_by(|left, right| left.path.cmp(&right.path));

    let total_bytes = records.iter().map(|record| record.bytes).sum::<u64>();
    let digest = snapshot_digest(env!("CARGO_PKG_VERSION"), &records);

    let mut marker = String::new();
    marker.push_str("DANGOMUSHI_RECOVERY_SNAPSHOT_V1\n");
    marker.push_str("kind=dangomushi-recovery-snapshot\n");
    marker.push_str("format=1\n");
    marker.push_str(&format!("app_version={}\n", env!("CARGO_PKG_VERSION")));
    marker.push_str("workspace_backup=recovery/workspace.dgmwbk\n");
    marker.push_str(&format!("file_count={}\n", records.len()));
    marker.push_str(&format!("total_bytes={total_bytes}\n"));
    marker.push_str(&format!("snapshot_sha256={}\n", hex_lower(&digest)));
    for record in &records {
        marker.push_str("entry=");
        marker.push_str(&hex_lower(record.path.as_bytes()));
        marker.push('|');
        marker.push_str(&record.bytes.to_string());
        marker.push('|');
        marker.push_str(&hex_lower(&record.sha256));
        marker.push('\n');
    }

    fs::write(snapshot.join(SNAPSHOT_MARKER), marker).expect("rewrite marker");
}

fn collect_records(root: &Path, current: &Path, records: &mut Vec<TestRecord>) {
    let mut entries = fs::read_dir(current)
        .expect("read directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("directory entries");
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("metadata");
        if metadata.is_dir() {
            collect_records(root, &path, records);
            continue;
        }
        if !metadata.is_file() {
            panic!("unexpected test entry");
        }

        let relative = path.strip_prefix(root).expect("relative");
        let text = relative
            .components()
            .map(|component| match component {
                Component::Normal(value) => value.to_str().expect("UTF-8").to_string(),
                _ => panic!("unexpected path component"),
            })
            .collect::<Vec<_>>()
            .join("/");
        if text == SNAPSHOT_MARKER {
            continue;
        }

        let bytes = fs::read(&path).expect("file bytes");
        let sha256: [u8; 32] = Sha256::digest(&bytes).into();
        records.push(TestRecord {
            path: text,
            bytes: bytes.len() as u64,
            sha256,
        });
    }
}

fn snapshot_digest(app_version: &str, records: &[TestRecord]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"DANGOMUSHI-RECOVERY-SNAPSHOT-DIGEST-V1\0");
    hasher.update(1u16.to_le_bytes());

    let version = app_version.as_bytes();
    hasher.update((version.len() as u32).to_le_bytes());
    hasher.update(version);
    hasher.update((records.len() as u64).to_le_bytes());

    for record in records {
        let path = record.path.as_bytes();
        hasher.update((path.len() as u32).to_le_bytes());
        hasher.update(path);
        hasher.update(record.bytes.to_le_bytes());
        hasher.update(record.sha256);
    }

    hasher.finalize().into()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[test]
fn valid_snapshot_round_trips_through_read_only_restore_verification() {
    let fixture = Fixture::new("valid");
    let before = collect_file_contents(&fixture.snapshot);

    let verified =
        recovery_snapshot::verify_snapshot(&fixture.snapshot).expect("verify recovery snapshot");
    assert_eq!(verified, fixture.created);

    let after = collect_file_contents(&fixture.snapshot);
    assert_eq!(before, after, "verification must not mutate snapshot payload");
}

#[test]
fn modified_payload_and_unexpected_directory_fail_closed() {
    let fixture = Fixture::new("tamper");
    fs::write(fixture.snapshot.join("Cargo.toml"), b"tampered").expect("tamper source");
    assert_eq!(
        recovery_snapshot::verify_snapshot(&fixture.snapshot).unwrap_err(),
        RecoverySnapshotError::IntegrityFailed
    );

    let fixture = Fixture::new("extra-directory");
    fs::create_dir(fixture.snapshot.join("unexpected-empty")).expect("extra directory");
    assert_eq!(
        recovery_snapshot::verify_snapshot(&fixture.snapshot).unwrap_err(),
        RecoverySnapshotError::UnsafeSnapshotLayout
    );
}

#[test]
fn unsupported_or_noncanonical_marker_fails_before_payload_is_trusted() {
    let fixture = Fixture::new("marker-version");
    let marker_path = fixture.snapshot.join(SNAPSHOT_MARKER);
    let marker = fs::read_to_string(&marker_path).expect("marker");
    fs::write(&marker_path, marker.replacen("format=1\n", "format=2\n", 1))
        .expect("unsupported format");
    assert_eq!(
        recovery_snapshot::verify_snapshot(&fixture.snapshot).unwrap_err(),
        RecoverySnapshotError::UnsupportedSnapshotVersion
    );

    let fixture = Fixture::new("marker-number");
    let marker_path = fixture.snapshot.join(SNAPSHOT_MARKER);
    let marker = fs::read_to_string(&marker_path).expect("marker");
    let line = marker
        .lines()
        .find(|line| line.starts_with("file_count="))
        .expect("file count");
    let count = line.trim_start_matches("file_count=");
    fs::write(
        &marker_path,
        marker.replacen(line, &format!("file_count=0{count}"), 1),
    )
    .expect("noncanonical count");
    assert_eq!(
        recovery_snapshot::verify_snapshot(&fixture.snapshot).unwrap_err(),
        RecoverySnapshotError::InvalidSnapshotMarker
    );
}

#[test]
fn embedded_workspace_backup_must_pass_its_own_archive_verifier() {
    let fixture = Fixture::new("nested-backup");
    let backup = fixture.snapshot.join("recovery/workspace.dgmwbk");
    let mut bytes = fs::read(&backup).expect("embedded backup");
    bytes[0] ^= 0x80;
    fs::write(&backup, bytes).expect("corrupt embedded backup");

    rewrite_marker_for_current_payload(&fixture.snapshot);

    assert_eq!(
        recovery_snapshot::verify_snapshot(&fixture.snapshot).unwrap_err(),
        RecoverySnapshotError::WorkspaceBackupUnavailable
    );
}

#[test]
fn public_verifier_has_no_restore_process_network_or_secret_authority() {
    let start = CORE
        .find("pub fn verify_snapshot")
        .expect("verify entry point");
    let end = CORE[start..]
        .find("\nfn populate_snapshot")
        .map(|offset| start + offset)
        .expect("verify entry end");
    let verify = &CORE[start..end];

    assert!(verify.contains("workspace_backup::verify_backup_file"));
    assert!(verify.contains("verify_payload_exact"));
    assert!(verify.contains("snapshot_digest_for_version"));

    for forbidden in [
        "restore_backup",
        "create_dir",
        "remove_dir",
        "OpenOptions",
        "write_all",
        "std::process",
        "Command::new",
        "std::net",
        "secret_backup",
        "vault_storage",
    ] {
        assert!(
            !verify.contains(forbidden),
            "restore verifier gained forbidden authority: {forbidden}"
        );
    }

    assert!(!SESSION.contains("recovery_snapshot_verified="));
    assert!(!SESSION.contains("recovery_snapshot_path="));
}

#[test]
fn cli_docs_and_roadmap_expose_verify_only_and_advance_once() {
    assert!(MAIN.contains(r#""verify" =>"#));
    assert!(MAIN.contains("recovery snapshot verify --snapshot <PATH>"));
    assert!(MAIN.contains("DANGOMUSHI RECOVERY SNAPSHOT VERIFIED"));
    assert!(!MAIN.contains(r#""restore" =>"#));

    for required in [
        "read-only trust gate",
        "self-consistent marker is not sufficient",
        "independent second-layer verification",
        "not publisher authenticity",
        "does not add recovery snapshot restore/extract behavior",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing restore-verification boundary: {required}"
        );
    }

    assert!(README.contains("read-only Restore Verification gate"));
    assert!(ROADMAP.contains("- [x] **Restore verification**"));
    assert!(ROADMAP.contains("- [x] **Portable release bundles per operating system**"));
}

fn collect_file_contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut output = Vec::new();
    collect_file_contents_recursive(root, root, &mut output);
    output.sort_by(|left, right| left.0.cmp(&right.0));
    output
}

fn collect_file_contents_recursive(
    root: &Path,
    current: &Path,
    output: &mut Vec<(String, Vec<u8>)>,
) {
    for entry in fs::read_dir(current).expect("read directory") {
        let entry = entry.expect("entry");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("metadata");
        if metadata.is_dir() {
            collect_file_contents_recursive(root, &path, output);
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("relative")
                .components()
                .map(|component| match component {
                    Component::Normal(value) => value.to_str().expect("UTF-8").to_string(),
                    _ => panic!("unexpected component"),
                })
                .collect::<Vec<_>>()
                .join("/");
            output.push((relative, fs::read(&path).expect("contents")));
        }
    }
}
