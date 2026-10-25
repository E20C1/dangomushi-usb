use dangomushi::{offline_reconstruction, recovery_snapshot, workspace, workspace_backup};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CORE: &str = include_str!("../src/offline_reconstruction.rs");
const MAIN: &str = include_str!("../src/main.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");
const SESSION: &str = include_str!("../src/session_state.rs");

fn temporary_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-frozen-rebuild-{label}-{}-{unique}",
        std::process::id()
    ))
}

#[test]
fn frozen_rebuild_uses_verified_staging_pinned_vendor_and_external_target() {
    for required in [
        "recovery_snapshot::verify_snapshot(staged_root)",
        "PINNED_RUST_TOOLCHAIN: &str = \"1.99.1\"",
        "VENDOR_CONFIG_FILE",
        ".arg(\"--frozen\")",
        ".arg(\"--release\")",
        ".arg(\"--target-dir\")",
        "&[\"--bin\", \"dangomushi\"]",
        "&[\"--features\", \"desktop\", \"--bin\", \"dangomushi-desktop\"]",
        "sanitize_cargo_environment",
        "CARGO_NET_OFFLINE",
        "recovery_snapshot::verify_snapshot(staged_root)",
        "write_rebuild_marker",
    ] {
        assert!(CORE.contains(required), "missing frozen rebuild boundary: {required}");
    }

    assert!(CORE.contains("Command::new(\"cargo\")"));
    assert!(CORE.contains("target/release/"));
}

#[test]
fn rebuild_keeps_later_authority_out_of_scope() {
    for forbidden in [
        "workspace_backup::restore_backup",
        "secret_backup",
        "vault_storage",
        "std::net",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "peer_lan",
        "server::",
    ] {
        assert!(
            !CORE.contains(forbidden),
            "frozen rebuild gained forbidden later authority: {forbidden}"
        );
    }

    assert!(!CORE.contains("Command::new(summary.cli"));
    assert!(!CORE.contains("Command::new(&summary.cli"));
    assert!(!SESSION.contains("frozen_rebuild_path="));
    assert!(!SESSION.contains("frozen_rebuild_sha256="));
}

#[test]
fn destination_and_environment_boundaries_are_explicit() {
    for required in [
        "DestinationExists",
        "DestinationInsideProtectedRoot",
        "UnsafeDestinationParent",
        "AmbientCargoConfig",
        "CARGO_HOME",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTUP_TOOLCHAIN",
        "CARGO_",
        ".cargo-home",
    ] {
        assert!(CORE.contains(required), "missing rebuild hardening: {required}");
    }

    assert!(CORE.contains("fs::remove_dir_all(&destination_path)"));
    assert!(CORE.contains("StagingChanged"));
}

#[test]
fn cli_exposes_only_fixed_rebuild_operation_and_roadmap_advances_once() {
    assert!(MAIN.contains(r#""rebuild" =>"#));
    assert!(MAIN.contains(
        "recovery snapshot rebuild --staging <PATH> --destination <PATH>"
    ));
    assert!(MAIN.contains("--staging is required for frozen reconstruction rebuild"));
    assert!(!MAIN.contains("--cargo-arg"));
    assert!(!MAIN.contains("--command"));

    assert!(ROADMAP.contains("- [x] **Frozen rebuild from staged source/vendor**"));
    assert!(ROADMAP.contains(
        "- [ ] **Recovered application smoke verification** — Exactly One Next Task"
    ));
    assert!(!ROADMAP.contains(
        "- [ ] **Workspace restore into a fresh destination** — Exactly One Next Task"
    ));
}

#[test]
fn documentation_states_process_execution_and_sandbox_limitations() {
    for required in [
        "Cargo/build.rs execution",
        "does not execute either rebuilt application",
        "outside the verified staging tree",
        "re-verifies the staged Recovery Snapshot after Cargo",
        "does not provide an OS-level network sandbox",
        "Recovered application smoke verification",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required),
            "missing rebuild limitation documentation: {required}"
        );
    }
    assert!(README.contains("frozen rebuild from staged source/vendor"));
}

#[test]
#[ignore = "dedicated CI exercises a real nested frozen rebuild"]
fn real_frozen_rebuild_from_verified_staging() {
    let source = std::env::current_dir().expect("repository root");
    assert!(source.join("vendor").is_dir(), "vendor/ must be generated first");

    let parent = temporary_path("real");
    let workspace_root = parent.join("workspace");
    let snapshot = parent.join("snapshot");
    let staging = parent.join("staging");
    let rebuild = parent.join("rebuild");
    fs::create_dir_all(&parent).expect("temporary parent");

    workspace::initialize(&workspace_root).expect("workspace");
    fs::write(
        workspace_root.join("data/documents/rebuild-proof.txt"),
        b"offline reconstruction proof",
    )
    .expect("workspace payload");
    let backup = workspace_backup::create_backup(&workspace_root)
        .expect("workspace backup")
        .relative_path;

    let created = recovery_snapshot::create_snapshot(
        &source,
        &workspace_root,
        &backup,
        &snapshot,
    )
    .expect("recovery snapshot");
    let staged = recovery_snapshot::stage_verified_snapshot(
        &snapshot,
        &staging,
        Some(&source),
        Some(&workspace_root),
    )
    .expect("verified staging");
    assert_eq!(created, staged);

    let summary = offline_reconstruction::frozen_rebuild(
        &staging,
        &rebuild,
        Some(&source),
        Some(&workspace_root),
    )
    .expect("frozen staged rebuild");

    assert_eq!(summary.source_snapshot, staged);
    assert_eq!(summary.toolchain, "1.99.1");
    assert!(summary.cli.bytes > 0);
    assert!(summary.desktop.bytes > 0);
    assert!(rebuild.join(&summary.cli.relative_path).is_file());
    assert!(rebuild.join(&summary.desktop.relative_path).is_file());
    assert!(rebuild
        .join(offline_reconstruction::REBUILD_MARKER)
        .is_file());
    assert_eq!(
        recovery_snapshot::verify_snapshot(&staging).expect("staging remains verified"),
        staged
    );
    assert!(!rebuild.join(".cargo-home").exists());

    fs::remove_dir_all(parent).expect("cleanup");
}
