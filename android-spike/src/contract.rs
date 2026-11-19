use dangomushi::{
    file_browser,
    session_state::{self, SessionState, SessionView},
    text_editor, workspace, workspace_backup,
};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeStep {
    pub label: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractReport {
    pub volume_root: PathBuf,
    pub test_root: PathBuf,
    pub steps: Vec<ProbeStep>,
}

impl ContractReport {
    pub fn passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|step| step.passed)
    }
}

fn run_step<T, E, F>(
    steps: &mut Vec<ProbeStep>,
    label: &'static str,
    operation: F,
) -> Result<T, ()>
where
    E: fmt::Display,
    F: FnOnce() -> Result<T, E>,
{
    match operation() {
        Ok(value) => {
            steps.push(ProbeStep {
                label,
                passed: true,
                detail: "PASS".to_string(),
            });
            Ok(value)
        }
        Err(error) => {
            steps.push(ProbeStep {
                label,
                passed: false,
                detail: error.to_string(),
            });
            Err(())
        }
    }
}

fn require(
    steps: &mut Vec<ProbeStep>,
    label: &'static str,
    condition: bool,
    failure: impl Into<String>,
) -> Result<(), ()> {
    if condition {
        steps.push(ProbeStep {
            label,
            passed: true,
            detail: "PASS".to_string(),
        });
        Ok(())
    } else {
        steps.push(ProbeStep {
            label,
            passed: false,
            detail: failure.into(),
        });
        Err(())
    }
}

pub fn run_storage_contract(volume_root: &Path) -> ContractReport {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let test_root = volume_root.join(format!(
        ".dangomushi-android-storage-contract-spike-{}-{unique}",
        std::process::id()
    ));

    let mut report = ContractReport {
        volume_root: volume_root.to_path_buf(),
        test_root: test_root.clone(),
        steps: Vec::new(),
    };

    let result = run_storage_contract_inner(volume_root, &test_root, &mut report.steps);

    match fs::remove_dir_all(&test_root) {
        Ok(()) => report.steps.push(ProbeStep {
            label: "cleanup temporary Workspace",
            passed: true,
            detail: "PASS".to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.steps.push(ProbeStep {
                label: "cleanup temporary Workspace",
                passed: true,
                detail: "PASS / already absent".to_string(),
            });
        }
        Err(error) => report.steps.push(ProbeStep {
            label: "cleanup temporary Workspace",
            passed: false,
            detail: error.to_string(),
        }),
    }

    if result.is_err() && report.steps.iter().all(|step| step.passed) {
        report.steps.push(ProbeStep {
            label: "contract execution",
            passed: false,
            detail: "probe aborted without a recorded failure".to_string(),
        });
    }

    report
}

fn run_storage_contract_inner(
    volume_root: &Path,
    test_root: &Path,
    steps: &mut Vec<ProbeStep>,
) -> Result<(), ()> {
    require(
        steps,
        "volume root is mounted directory",
        volume_root.is_dir(),
        "selected StorageVolume directory is not available",
    )?;

    let canonical_volume = run_step(steps, "canonicalize volume root", || {
        fs::canonicalize(volume_root)
    })?;

    run_step(steps, "create temporary Workspace root", || {
        fs::create_dir(test_root)
    })?;

    run_step(steps, "initialize DANGOMUSHI Workspace", || {
        workspace::initialize(test_root)
    })?;

    require(
        steps,
        "Workspace doctor",
        workspace::doctor(test_root).ok(),
        "Workspace marker or required directories are missing",
    )?;

    let canonical_workspace = run_step(steps, "canonicalize Workspace root", || {
        fs::canonicalize(test_root)
    })?;

    require(
        steps,
        "Workspace remains inside selected volume",
        canonical_workspace.starts_with(&canonical_volume),
        "canonical Workspace escaped selected volume",
    )?;

    let document_relative = PathBuf::from("data/documents/probe.txt");
    let document_path = test_root.join(&document_relative);
    run_step(steps, "create_new + write + file sync_all", || {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&document_path)?;
        file.write_all(b"before\n")?;
        file.sync_all()
    })?;

    let read_back = run_step(steps, "direct std::fs read", || fs::read(&document_path))?;
    require(
        steps,
        "direct read is byte exact",
        read_back == b"before\n",
        "direct read did not match bytes written",
    )?;

    let metadata = run_step(steps, "regular-file metadata", || {
        fs::symlink_metadata(&document_path)
    })?;
    require(
        steps,
        "metadata type/length",
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 7,
        "metadata did not describe the expected regular file",
    )?;

    let canonical_document = run_step(steps, "canonicalize Workspace file", || {
        fs::canonicalize(&document_path)
    })?;
    require(
        steps,
        "canonical file remains in Workspace",
        canonical_document.starts_with(&canonical_workspace),
        "canonical file escaped temporary Workspace",
    )?;

    let editor_before = run_step(steps, "Text Editor read boundary", || {
        text_editor::read_text(test_root, &document_relative)
    })?;
    require(
        steps,
        "Editor initial content",
        editor_before == "before\n",
        "Text Editor returned unexpected content",
    )?;

    run_step(steps, "Text Editor explicit save + sync", || {
        text_editor::save_text(test_root, &document_relative, "after\n")
    })?;
    let editor_after = run_step(steps, "Text Editor reopen", || {
        text_editor::read_text(test_root, &document_relative)
    })?;
    require(
        steps,
        "Editor saved content",
        editor_after == "after\n",
        "Text Editor save did not survive reopen",
    )?;

    run_step(steps, "Editor recovery draft write", || {
        text_editor::write_recovery(test_root, &document_relative, "draft\n")
    })?;
    let recovery = run_step(steps, "Editor recovery draft reopen", || {
        text_editor::latest_recovery(test_root, &document_relative)
    })?;
    require(
        steps,
        "Editor recovery content",
        recovery.as_ref().is_some_and(|draft| draft.text == "draft\n"),
        "latest recovery draft did not match",
    )?;
    run_step(steps, "Editor recovery cleanup", || {
        text_editor::clear_recovery(test_root, &document_relative)
    })?;

    let renamed_relative = PathBuf::from("data/documents/probe-renamed.txt");
    let renamed_path = test_root.join(&renamed_relative);
    run_step(steps, "same-volume rename", || {
        fs::rename(&document_path, &renamed_path)
    })?;
    require(
        steps,
        "renamed file exists",
        renamed_path.is_file() && !document_path.exists(),
        "rename did not produce the expected filesystem state",
    )?;

    let entries = run_step(steps, "Workspace directory enumeration", || {
        file_browser::list_directory(test_root, Path::new("data/documents"))
    })?;
    require(
        steps,
        "File Browser sees renamed file",
        entries
            .iter()
            .any(|entry| entry.relative_path == renamed_relative),
        "File Browser did not enumerate the renamed file",
    )?;

    let session = SessionState {
        view: SessionView::Editor,
        browser_dir: PathBuf::from("data/documents"),
        browser_selected: Some(renamed_relative.clone()),
        editor_path: Some(renamed_relative.clone()),
    };
    run_step(steps, "session save", || session_state::save(test_root, &session))?;
    let loaded_session = run_step(steps, "session load", || session_state::load(test_root))?;
    require(
        steps,
        "session round-trip",
        loaded_session.as_ref() == Some(&session),
        "session state changed across save/load",
    )?;

    let removable = test_root.join("data/remove-me.tmp");
    run_step(steps, "create_new removable file", || {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&removable)?;
        file.write_all(b"remove")?;
        file.sync_all()
    })?;
    run_step(steps, "remove file", || fs::remove_file(&removable))?;
    require(
        steps,
        "removed file is absent",
        !removable.exists(),
        "removed file still exists",
    )?;

    let backup = run_step(steps, "Generic Workspace Backup create", || {
        workspace_backup::create_backup(test_root)
    })?;
    let summary = run_step(steps, "Generic Workspace Backup verify", || {
        workspace_backup::verify_backup(test_root, &backup.relative_path)
    })?;
    require(
        steps,
        "backup includes managed file data",
        summary.files >= 1 && summary.archive_bytes > 0,
        "verified backup did not contain the expected managed data",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_filesystem_exercises_same_storage_contract() {
        let parent = std::env::temp_dir();
        let report = run_storage_contract(&parent);
        assert!(report.passed(), "{report:#?}");
    }
}
