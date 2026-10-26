const CORE: &str = include_str!("../src/offline_reconstruction.rs");
const BACKUP: &str = include_str!("../src/workspace_backup.rs");
const MAIN: &str = include_str!("../src/main.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");
const SESSION: &str = include_str!("../src/session_state.rs");

#[test]
fn final_verifier_reuses_existing_snapshot_rebuild_and_smoke_evidence() {
    let verify = CORE
        .split("pub fn verify_reconstruction")
        .nth(1)
        .expect("final reconstruction verifier")
        .split("fn checked_verification_root")
        .next()
        .expect("verifier body");

    for required in [
        "checked_verification_root",
        "roots_overlap",
        "verify_fresh_reconstruction_layout",
        "smoke_verify_rebuild",
        "workspace_backup::verify_backup_file",
        "workspace_backup::verify_backup_file_matches_workspace",
        "recovery_snapshot::verify_snapshot",
        "WorkspaceBackupMismatch",
        "WorkspaceContentsMismatch",
        "StagingChanged",
    ] {
        assert!(verify.contains(required), "missing final verification evidence: {required}");
    }
}

#[test]
fn restored_workspace_comparison_is_read_only_and_exact_for_managed_scope() {
    for required in [
        "verify_backup_file_matches_workspace",
        "verify_archive_path_against_workspace",
        "verify_archive_path_internal(backup_path, None, Some(workspace_root))",
        "verify_comparison_directory",
        "comparison_buffer",
        "WorkspaceContentsMismatch",
        "verify_comparison_tree_exact",
        "collect_comparison_entries",
    ] {
        assert!(BACKUP.contains(required), "missing managed Workspace comparison: {required}");
    }

    assert!(BACKUP.contains("if &observed != expected"));
}

#[test]
fn final_verifier_requires_fresh_only_non_managed_layout() {
    let layout = CORE
        .split("fn verify_fresh_reconstruction_layout")
        .nth(1)
        .expect("fresh layout verifier")
        .split("fn checked_restore_root")
        .next()
        .expect("layout verifier body");

    for required in [
        ".dangomushi-workspace",
        "backups",
        "state",
        "autosave",
        "workspace::doctor",
        "WorkspaceLayoutMismatch",
    ] {
        assert!(layout.contains(required), "missing fresh-layout gate: {required}");
    }
}

#[test]
fn reconstruction_verification_does_not_expand_secret_or_network_authority() {
    let verify = CORE
        .split("pub fn verify_reconstruction")
        .nth(1)
        .expect("final reconstruction verifier")
        .split("fn checked_verification_root")
        .next()
        .expect("verifier body");

    for forbidden in [
        "secret_backup::",
        "vault_storage::",
        "MasterPassphrase",
        "RecoveryCode",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "peer_lan::",
        "internet_p2p",
        "restore_workspace_into_fresh_destination(",
        "frozen_rebuild(",
    ] {
        assert!(
            !verify.contains(forbidden),
            "final verifier gained unrelated authority: {forbidden}"
        );
    }

    for forbidden in [
        "reconstruction_verified=",
        "reconstruction_workspace=",
        "reconstruction_rebuild=",
    ] {
        assert!(!SESSION.contains(forbidden));
    }
}

#[test]
fn cli_exposes_fixed_final_verification_surface() {
    for required in [
        r#""verify-reconstruction" =>"#,
        "offline_reconstruction::verify_reconstruction",
        "--staging",
        "--rebuild",
        "--restored-workspace",
        "DANGOMUSHI RECONSTRUCTION VERIFIED",
    ] {
        assert!(MAIN.contains(required), "missing final verification CLI: {required}");
    }

    for forbidden in ["--secret-backup", "--passphrase", "--merge", "--overwrite", "--command"] {
        assert!(
            !MAIN.contains(&format!("verify-reconstruction {forbidden}")),
            "final verification must not expose {forbidden}"
        );
    }
}

#[test]
fn docs_close_m6_without_implicitly_authorizing_m7() {
    assert!(ROADMAP.contains("- [x] **Reconstruction verification**"));
    assert!(ROADMAP.contains("M6 Offline reconstruction is complete."));
    assert!(ROADMAP.contains("No post-M6 implementation task is authorized"));
    assert!(!ROADMAP.contains("- [ ] **Reconstruction verification**"));

    for required in [
        "byte-compared",
        "fresh-only non-managed layout",
        "not publisher authenticity",
        "Secret Backup",
        "No post-M6 implementation task",
    ] {
        assert!(
            DOC.contains(required)
                || ARCHITECTURE.contains(required)
                || README.contains(required),
            "missing final M6 documentation: {required}"
        );
    }
}
