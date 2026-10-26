use dangomushi::{workspace, workspace_backup};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CORE: &str = include_str!("../src/offline_reconstruction.rs");
const BACKUP: &str = include_str!("../src/workspace_backup.rs");
const MAIN: &str = include_str!("../src/main.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const DOC: &str = include_str!("../docs/OFFLINE_RECONSTRUCTION.md");
const README: &str = include_str!("../README.md");
const SESSION: &str = include_str!("../src/session_state.rs");

fn temporary_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dangomushi-fresh-restore-{label}-{}-{unique}",
        std::process::id()
    ))
}

#[test]
fn generic_backup_external_restore_creates_fresh_workspace_and_preserves_scope() {
    let source = temporary_root("source");
    workspace::initialize(&source).expect("initialize source");
    fs::create_dir_all(source.join("state/autosave")).expect("autosave");
    fs::write(source.join("data/documents/note.txt"), "restored\n").expect("document");
    fs::write(source.join("identity/public-state.txt"), "identity").expect("identity");
    fs::write(source.join("config/settings.toml"), "mode = \"recovered\"\n").expect("config");
    fs::write(source.join("state/autosave/draft.txt"), "draft").expect("draft");
    fs::write(source.join("state/session.v1"), "excluded-session").expect("session");
    fs::write(
        source.join("state/device_permissions.v1.json"),
        "{\"excluded\":true}\n",
    )
    .expect("permission policy");

    let created = workspace_backup::create_backup(&source).expect("create backup");
    let backup_path = source.join(&created.relative_path);
    let expected = workspace_backup::verify_backup(&source, &created.relative_path)
        .expect("verify source backup");

    let destination = temporary_root("destination");
    let restored = workspace_backup::restore_backup_file_into_fresh_workspace(
        &backup_path,
        &destination,
    )
    .expect("restore fresh Workspace");

    assert_eq!(restored, expected);
    assert!(workspace::doctor(&destination).ok());
    assert_eq!(
        fs::read_to_string(destination.join("data/documents/note.txt")).expect("document"),
        "restored\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("identity/public-state.txt")).expect("identity"),
        "identity"
    );
    assert!(!destination.join("state/session.v1").exists());
    assert!(!destination.join("state/device_permissions.v1.json").exists());
    assert_eq!(
        fs::read_dir(destination.join("backups"))
            .expect("backups")
            .count(),
        0
    );

    fs::remove_dir_all(source).expect("cleanup source");
    fs::remove_dir_all(destination).expect("cleanup destination");
}

#[test]
fn fresh_restore_runtime_orders_smoke_and_backup_verification_before_destination_mutation() {
    let function = CORE
        .split("pub fn restore_workspace_into_fresh_destination")
        .nth(1)
        .expect("fresh restore function")
        .split("fn checked_restore_root")
        .next()
        .expect("function body");

    let destination_check = function
        .find("checked_fresh_restore_destination")
        .expect("destination check");
    let smoke = function
        .find("smoke_verify_rebuild")
        .expect("smoke gate");
    let backup_verify = function
        .find("workspace_backup::verify_backup_file")
        .expect("backup verification");
    let restore = function
        .find("workspace_backup::restore_backup_file_into_fresh_workspace")
        .expect("fresh restore call");
    let post_snapshot = function
        .rfind("recovery_snapshot::verify_snapshot")
        .expect("post restore staging verification");

    assert!(destination_check < smoke);
    assert!(smoke < backup_verify);
    assert!(backup_verify < restore);
    assert!(restore < post_snapshot);

    for required in [
        r#"join("recovery").join("workspace.dgmwbk")"#,
        "smoke.source_snapshot.workspace_backup",
        "DestinationInsideProtectedRoot",
        "fs::remove_dir_all(&destination_path)",
        "StagingChanged",
    ] {
        assert!(function.contains(required), "missing restore gate: {required}");
    }
}

#[test]
fn generic_backup_restore_core_is_reused_not_reimplemented() {
    for required in [
        "restore_backup_file_into_fresh_workspace",
        "restore_archive_into_workspace",
        "verify_archive_path(backup_path, None)",
        "validate_restore_targets",
        "merge_for_workspace_restore",
        "replace_managed_roots",
        "rollback_replacements",
        "hash_backup_source",
        "workspace::doctor",
    ] {
        assert!(BACKUP.contains(required), "missing Generic Backup reuse: {required}");
    }

    assert!(BACKUP.contains("FreshRestoreDestinationExists"));
    assert!(BACKUP.contains("FreshRestoreDestinationUnsafe"));
}

#[test]
fn cli_exposes_only_fresh_workspace_restore_surface() {
    for required in [
        r#""restore-workspace" =>"#,
        "restore_workspace_into_fresh_destination",
        "--staging",
        "--rebuild",
        "--destination",
        "--source",
        "--workspace",
        "DANGOMUSHI FRESH WORKSPACE RESTORED",
    ] {
        assert!(MAIN.contains(required), "missing restore CLI surface: {required}");
    }

    for forbidden in ["--merge", "--overwrite", "--secret-backup", "--vault", "--passphrase"] {
        assert!(
            !MAIN.contains(&format!("restore-workspace {forbidden}")),
            "fresh restore must not expose {forbidden}"
        );
    }
}

#[test]
fn restored_scope_does_not_smuggle_secret_or_session_authority() {
    let production = CORE
        .split("#[cfg(test)]")
        .next()
        .expect("production reconstruction core");
    let restore = production
        .split("pub fn restore_workspace_into_fresh_destination")
        .nth(1)
        .expect("restore function");

    for forbidden in [
        "secret_backup::",
        "vault_storage::",
        "RecoveryCode",
        "MasterPassphrase",
        "TcpListener",
        "peer_lan::",
        "internet_p2p",
    ] {
        assert!(
            !restore.contains(forbidden),
            "fresh restore must not add authority: {forbidden}"
        );
    }

    for forbidden in [
        "reconstruction_restore=",
        "fresh_workspace_restore=",
        "recovery_destination=",
    ] {
        assert!(!SESSION.contains(forbidden));
    }
}

#[test]
fn docs_and_roadmap_advance_exactly_to_reconstruction_verification() {
    for required in [
        "reruns the complete recovered-application smoke gate",
        "must not exist",
        "Generic Workspace Backup",
        "Secret Backup remains separate",
        "never overwrites or merges an existing Workspace",
    ] {
        assert!(
            DOC.contains(required) || ARCHITECTURE.contains(required) || README.contains(required),
            "missing fresh restore documentation: {required}"
        );
    }

    assert!(ROADMAP.contains("- [x] **Workspace restore into a fresh destination**"));
    assert!(ROADMAP.contains(
        "- [x] **Reconstruction verification**"
    ));
}
