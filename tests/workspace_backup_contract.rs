const SOURCE: &str = include_str!("../src/workspace_backup.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const SECRET_BOUNDARY: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");

#[test]
fn generic_workspace_backup_has_versioned_streaming_integrity_boundary() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        r#"b"DGMWBK01""#,
        r#"b"DGMEND01""#,
        "Sha256",
        "IO_BUFFER_BYTES",
        "MAX_BACKUP_ENTRIES",
        "MAX_PATH_COMPONENTS",
        "MAX_COMPONENT_BYTES",
        "MAX_SINGLE_FILE_BYTES",
        "MAX_TOTAL_FILE_BYTES",
        "write_archive",
        "verify_archive_path",
        "file_hasher",
        "archive_hasher",
        "create_new(true)",
        "sync_all",
    ] {
        assert!(
            production.contains(required),
            "missing generic backup format/integrity evidence: {required}"
        );
    }

    assert!(!production.contains("crate::secret_backup"));
    assert!(!production.contains("crate::vault_storage"));
    assert!(!production.contains("MasterPassphrase"));
    assert!(!production.contains("RecoveryCode"));
}

#[test]
fn generic_backup_scope_excludes_backups_and_session_but_includes_autosave() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        r#"&["data"]"#,
        r#"&["repositories"]"#,
        r#"&["identity"]"#,
        r#"&["config"]"#,
        r#"&["state", "autosave"]"#,
        "BACKUPS_DIR",
        "UnsupportedEntry",
        "NonUtf8Path",
        "fs::symlink_metadata",
        "validate_component_text",
    ] {
        assert!(
            production.contains(required),
            "missing generic backup scope/path evidence: {required}"
        );
    }

    assert!(!production.contains(r#"&["backups"]"#));
    assert!(!production.contains(r#"&["state", "session.v1"]"#));
}

#[test]
fn restore_verifies_before_replace_and_keeps_rollback_path() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    let restore = production
        .split("pub fn restore_backup")
        .nth(1)
        .expect("restore function");

    let verify_position = restore
        .find("verify_archive_path(&backup_path, Some(&staging_root))")
        .expect("verified extraction");
    let replace_position = restore
        .find("replace_managed_roots")
        .expect("managed-root replacement");
    assert!(
        verify_position < replace_position,
        "archive must verify/extract before managed roots are replaced"
    );

    for required in [
        ".restore-staging-",
        ".restore-rollback-",
        "ensure_no_restore_artifacts",
        "validate_restore_targets",
        "rollback_replacements",
        "fs::rename",
        "RestoreTargetUnsafe",
        "RestoreFailed",
    ] {
        assert!(
            production.contains(required),
            "missing restore staging/rollback evidence: {required}"
        );
    }
}

#[test]
fn native_ui_requires_explicit_restore_and_resets_filesystem_state() {
    let entry = REGISTRY
        .split("id: ToolId::Backup,")
        .nth(1)
        .expect("backup registry entry")
        .split("},")
        .next()
        .expect("backup registry block");

    assert!(entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::Backup =>"));
    assert!(UI.contains("View::Backup"));
    assert!(UI.contains("fn backup_view"));
    assert!(UI.contains("[CREATE BACKUP]"));
    assert!(UI.contains("[VERIFY BACKUP]"));
    assert!(UI.contains("[RESTORE SELECTED BACKUP]"));
    assert!(UI.contains(r#"restore_confirmation == "RESTORE""#));
    assert!(UI.contains("!self.editor.dirty"));
    assert!(UI.contains("self.editor.pending_recovery.is_none()"));
    assert!(UI.contains("reset_after_workspace_restore"));
    assert!(UI.contains("persist_dirty_editor_recovery_for_backup"));

    assert!(SESSION.contains("Backup"));
    assert!(SESSION.contains(r#""backup""#));
    for forbidden in [
        "backup_selected=",
        "backup_summary=",
        "restore_confirmation=",
        "restore_result=",
        "backup_path=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist BACKUP field: {forbidden}"
        );
    }
}

#[test]
fn generic_backup_stays_outside_secret_boundary_and_completes_m2() {
    assert!(SECRET_BOUNDARY.contains(
        "Generic BACKUP does not replace or wrap the M2-S Secret Backup / Restore flow"
    ));
    assert!(ROADMAP.contains("- [x] **Backup and restore workflow**"));
    assert!(ROADMAP.contains("- [x] **Explicit LAN enable control**"));
    assert!(ROADMAP.contains("- [x] **Device pairing**"));
    assert!(ROADMAP.contains(
        "- [ ] **Permission scopes** — Exactly One Next Task"
    ));
}
