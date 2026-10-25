const CORE: &str = include_str!("../src/offline_reconstruction.rs");
const DESKTOP: &str = include_str!("../src/bin/dangomushi-desktop.rs");
const MAIN: &str = include_str!("../src/main.rs");
const CI: &str = include_str!("../.github/workflows/ci.yml");
const SMOKE_SCRIPT: &str = include_str!("../scripts/verify-smoke-entrypoints.py");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");
const SESSION: &str = include_str!("../src/session_state.rs");

#[test]
fn smoke_requires_verified_staging_and_integrity_recorded_rebuild_artifacts() {
    for required in [
        "recovery_snapshot::verify_snapshot(staged_root)",
        "parse_rebuild_marker",
        "source_snapshot_sha256 != before_staging.sha256",
        "PINNED_RUST_TOOLCHAIN",
        "std::env::consts::OS",
        "std::env::consts::ARCH",
        "target/release/",
        "verify_recorded_artifact",
        "marker_after != marker_before",
        "recovery_snapshot::verify_snapshot(&canonical_staging)",
    ] {
        assert!(CORE.contains(required), "missing smoke trust gate: {required}");
    }

    assert!(CORE.contains("MAX_REBUILD_MARKER_BYTES"));
    assert!(CORE.contains("MAX_REBUILD_BINARY_BYTES"));
}

#[test]
fn smoke_executes_only_fixed_cli_and_non_gui_desktop_entrypoints() {
    for required in [
        r#"&["version"]"#,
        r#"&["--recovery-smoke-check"]"#,
        "dangomushi {app_version}",
        "dangomushi-desktop {app_version} recovery-smoke-ok",
    ] {
        assert!(CORE.contains(required), "missing fixed smoke behavior: {required}");
    }

    assert!(DESKTOP.contains("--recovery-smoke-check"));
    assert!(DESKTOP.contains("recovery-smoke-ok"));
    let smoke_index = DESKTOP
        .find("--recovery-smoke-check")
        .expect("desktop smoke flag");
    let ui_index = DESKTOP
        .find("dangomushi::native_ui::run()")
        .expect("normal native UI launch");
    assert!(
        smoke_index < ui_index,
        "desktop smoke must return before normal GUI initialization"
    );

    for forbidden in ["--command", "--args", "--program", "--workspace"] {
        assert!(
            !CORE.contains(&format!("args.next() {forbidden}")),
            "smoke must not accept arbitrary execution surface: {forbidden}"
        );
    }
}

#[test]
fn smoke_uses_bounded_rebuild_local_process_state() {
    for required in [
        "SMOKE_SANDBOX_DIR",
        ".dangomushi-recovery-smoke",
        "SMOKE_TIMEOUT: Duration = Duration::from_secs(15)",
        "MAX_SMOKE_OUTPUT_BYTES",
        "Stdio::null()",
        "HOME",
        "USERPROFILE",
        "XDG_CONFIG_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "TMPDIR",
        "DANGOMUSHI_",
        "fs::remove_dir_all(&sandbox)",
    ] {
        assert!(CORE.contains(required), "missing smoke process bound: {required}");
    }

    assert!(CORE.contains("ProcessTimedOut"));
    assert!(CORE.contains("UnexpectedOutput"));
    assert!(CORE.contains("ProcessFailed"));
}

#[test]
fn smoke_does_not_pull_workspace_restore_secret_or_network_authority_forward() {
    let smoke = CORE
        .split("pub fn smoke_verify_rebuild")
        .nth(1)
        .expect("smoke function")
        .split("pub struct FreshWorkspaceRestoreSummary")
        .next()
        .expect("smoke function end");

    for forbidden in [
        "workspace_backup::restore_backup",
        "secret_backup",
        "vault_storage",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "peer_lan",
        "server::",
    ] {
        assert!(
            !smoke.contains(forbidden),
            "smoke gained forbidden later authority: {forbidden}"
        );
    }

    assert!(!SESSION.contains("recovery_smoke_path="));
    assert!(!SESSION.contains("recovery_smoke_result="));
}

#[test]
fn cli_and_ci_expose_only_the_bounded_smoke_flow() {
    assert!(MAIN.contains(r#""smoke" =>"#));
    assert!(MAIN.contains(
        "recovery snapshot smoke --staging <PATH> --rebuild <PATH>"
    ));
    assert!(MAIN.contains("smoke_verify_rebuild"));
    assert!(!MAIN.contains("--smoke-command"));

    assert!(CI.contains("Verify recovered smoke entrypoints"));
    assert!(CI.contains("python scripts/verify-smoke-entrypoints.py"));
    assert!(CI.contains(
        "Exercise verified staging, frozen rebuild, and recovered smoke verification"
    ));

    for required in [
        "dangomushi-desktop",
        "--recovery-smoke-check",
        "timeout=15",
        "TemporaryDirectory",
    ] {
        assert!(SMOKE_SCRIPT.contains(required), "CI smoke helper missing: {required}");
    }
}

#[test]
fn documentation_states_execution_limitations_and_advances_once() {
    for required in [
        "not an operating-system sandbox",
        "publisher authenticity",
        "15-second timeout",
        "does not restore",
        "Workspace restore into a fresh destination",
    ] {
        assert!(
            DOC.contains(required)
                || ARCHITECTURE.contains(required)
                || README.contains(required),
            "missing smoke limitation documentation: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Recovered application smoke verification**"));
    assert!(ROADMAP.contains("- [x] **Workspace restore into a fresh destination**"));
    assert!(ROADMAP.contains(
        "- [ ] **Reconstruction verification** — Exactly One Next Task"
    ));
}
