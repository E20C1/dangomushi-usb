const SOURCE: &str = include_str!("../src/repository_git.rs");
const UI: &str = include_str!("../src/native_ui.rs");
const REGISTRY: &str = include_str!("../src/tool_registry.rs");
const SESSION: &str = include_str!("../src/session_state.rs");
const ROADMAP: &str = include_str!("../docs/ROADMAP.md");
const SECRET_BOUNDARY: &str = include_str!("../docs/SECRET_DATA_BOUNDARY.md");

#[test]
fn repository_git_core_uses_fixed_bounded_read_only_process_boundary() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        r#"Command::new("git")"#,
        r#""--no-pager""#,
        r#""--no-optional-locks""#,
        r#""--no-replace-objects""#,
        r#""core.fsmonitor=false""#,
        r#""submodule.recurse=false""#,
        r#""log.showSignature=false""#,
        "Stdio::null()",
        r#""GIT_OPTIONAL_LOCKS""#,
        r#""GIT_TERMINAL_PROMPT""#,
        r#""GIT_DIR""#,
        r#""GIT_WORK_TREE""#,
        r#""GIT_INDEX_FILE""#,
        r#""GIT_OBJECT_DIRECTORY""#,
        r#""GIT_EXTERNAL_DIFF""#,
        r#""GIT_PAGER""#,
        r#""GIT_EDITOR""#,
        r#""GIT_ASKPASS""#,
        r#""GIT_SSH_COMMAND""#,
        r#""GIT_CONFIG_PARAMETERS""#,
        r#""GIT_REDIRECT_STDIN""#,
        "MAX_GIT_OUTPUT_BYTES",
        "MAX_GIT_RUNTIME",
        "drain_bounded",
        "child.try_wait",
        "child.kill",
    ] {
        assert!(
            production.contains(required),
            "missing repository process boundary evidence: {required}"
        );
    }

    assert!(production.contains("fn run_git("));
    assert!(!production.contains("pub fn run_git("));
    assert!(!production.contains("safe.directory"));

    for forbidden in [
        r#"Command::new("sh")"#,
        r#"Command::new("bash")"#,
        r#"Command::new("cmd")"#,
        r#"Command::new("cmd.exe")"#,
        r#"Command::new("powershell")"#,
        r#""clone""#,
        r#""fetch""#,
        r#""pull""#,
        r#""push""#,
        r#""add""#,
        r#""commit""#,
        r#""checkout""#,
        r#""switch""#,
        r#""restore""#,
        r#""reset""#,
        r#""clean""#,
        r#""stash""#,
        r#""merge""#,
        r#""rebase""#,
        r#""cherry-pick""#,
    ] {
        assert!(
            !production.contains(forbidden),
            "mutating/network/shell Git surface must remain absent: {forbidden}"
        );
    }
}

#[test]
fn repository_operations_are_exactly_status_log_and_diff_stat_views() {
    let production = SOURCE
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    for required in [
        r#""status""#,
        r#""--porcelain=v1""#,
        r#""--branch""#,
        r#""--ignore-submodules=all""#,
        r#""--no-renames""#,
        r#""log""#,
        r#""--no-decorate""#,
        r#""--no-notes""#,
        r#""diff""#,
        r#""--no-ext-diff""#,
        r#""--no-textconv""#,
        r#""--stat""#,
        r#""--cached""#,
    ] {
        assert!(
            production.contains(required),
            "missing fixed Git operation evidence: {required}"
        );
    }

    assert!(production.contains("validate_repository_relative"));
    assert!(production.contains("REPOSITORIES_DIR"));
    assert!(production.contains("fs::symlink_metadata"));
    assert!(production.contains("canonical.starts_with(&repositories_root)"));
}

#[test]
fn native_ui_dispatches_ready_repositories_without_session_git_payload() {
    let entry = REGISTRY
        .split("id: ToolId::Repositories,")
        .nth(1)
        .expect("repositories registry entry")
        .split("},")
        .next()
        .expect("repositories registry block");

    assert!(entry.contains("status: ToolStatus::Ready"));
    assert!(UI.contains("ToolId::Repositories =>"));
    assert!(UI.contains("View::Repositories"));
    assert!(UI.contains("fn repositories_view"));
    assert!(UI.contains("[STATUS]"));
    assert!(UI.contains("[RECENT COMMITS]"));
    assert!(UI.contains("[WORKTREE DIFF STAT]"));
    assert!(UI.contains("[STAGED DIFF STAT]"));
    assert!(UI.contains("No network, shell, arbitrary argv"));

    assert!(SESSION.contains("Repositories"));
    assert!(SESSION.contains(r#""repositories""#));

    for forbidden in [
        "repository_path=",
        "git_status=",
        "git_diff=",
        "git_log=",
        "branch_name=",
        "commit_subject=",
    ] {
        assert!(
            !SESSION.contains(forbidden),
            "session schema must not persist REPOSITORIES field: {forbidden}"
        );
    }
}

#[test]
fn repositories_stay_outside_secret_boundary_after_backup_advances() {
    assert!(SECRET_BOUNDARY.contains(
        "MUST NOT be intentionally routed through repository files, commit messages, Git output, or the generic REPOSITORIES UI"
    ));
    assert!(ROADMAP.contains("- [x] **Repository status and safe Git operations**"));
    assert!(ROADMAP.contains("- [x] **Backup and restore workflow**"));
    assert!(ROADMAP.contains(
        "- [ ] **Explicit LAN enable control** — Exactly One Next Task"
    ));
    assert!(ROADMAP.contains("- [x] **Backup and restore workflow**"));
}
