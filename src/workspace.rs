use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const REQUIRED_DIRS: &[&str] = &[
    "data/documents",
    "data/exports",
    "data/received",
    "repositories",
    "backups",
    "state",
    "identity",
    "config",
];

const MARKER_FILE: &str = ".dangomushi-workspace";

#[derive(Debug)]
pub struct Check {
    pub label: String,
    pub ok: bool,
}

#[derive(Debug)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WorkspaceSummary {
    pub initialized: bool,
    pub present_dirs: usize,
    pub total_dirs: usize,
}

pub fn initialize(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;

    for relative in REQUIRED_DIRS {
        fs::create_dir_all(root.join(relative))?;
    }

    let marker = root.join(MARKER_FILE);
    if !marker.exists() {
        fs::write(
            marker,
            "{\n  \"format\": 1,\n  \"kind\": \"dangomushi-workspace\"\n}\n",
        )?;
    }

    Ok(())
}

pub fn doctor(root: &Path) -> DoctorReport {
    let mut checks = Vec::with_capacity(REQUIRED_DIRS.len() + 2);

    checks.push(Check {
        label: format!("root directory ({})", root.display()),
        ok: root.is_dir(),
    });

    checks.push(Check {
        label: MARKER_FILE.to_string(),
        ok: root.join(MARKER_FILE).is_file(),
    });

    checks.extend(REQUIRED_DIRS.iter().map(|relative| Check {
        label: relative.to_string(),
        ok: root.join(relative).is_dir(),
    }));

    DoctorReport { checks }
}

pub fn summary(root: &Path) -> WorkspaceSummary {
    let present_dirs = REQUIRED_DIRS
        .iter()
        .filter(|relative| root.join(relative).is_dir())
        .count();

    WorkspaceSummary {
        initialized: root.is_dir() && root.join(MARKER_FILE).is_file(),
        present_dirs,
        total_dirs: REQUIRED_DIRS.len(),
    }
}

pub fn default_workspace_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("DANGOMUSHI_WORKSPACE") {
        return PathBuf::from(configured);
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let is_app_dir = parent
                .file_name()
                .map(|name| name.to_string_lossy().eq_ignore_ascii_case("app"))
                .unwrap_or(false);

            if is_app_dir {
                if let Some(portable_root) = parent.parent() {
                    return portable_root.join("workspace");
                }
            }
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("workspace")
}

pub fn canonical_or_original(root: &Path) -> PathBuf {
    fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dangomushi-test-{}-{unique}", std::process::id()))
    }

    #[test]
    fn initialize_creates_expected_layout() {
        let root = temporary_workspace();
        initialize(&root).expect("initialize");

        let report = doctor(&root);
        assert!(report.ok());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn missing_workspace_is_not_healthy() {
        let root = temporary_workspace();
        let report = doctor(&root);
        assert!(!report.ok());
    }
}
