use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_REPOSITORIES: usize = 512;
pub const MAX_STATUS_ENTRIES: usize = 500;
pub const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_GIT_RUNTIME: Duration = Duration::from_secs(15);
pub const RECENT_COMMIT_LIMIT: usize = 20;

const REPOSITORIES_DIR: &str = "repositories";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryEntry {
    pub name: String,
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryStatusEntry {
    pub code: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryStatus {
    pub branch: String,
    pub entries: Vec<RepositoryStatusEntry>,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub output_truncated: bool,
    pub entry_limit_reached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTextOutput {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryGitError {
    RepositoriesRootUnavailable,
    UnsafeRepositoriesRoot,
    InvalidRepositoryPath,
    NotRepository,
    GitUnavailable,
    GitTimedOut,
    GitFailed,
    Io,
}

impl fmt::Display for RepositoryGitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::RepositoriesRootUnavailable => "repositories root is unavailable",
            Self::UnsafeRepositoriesRoot => "repositories root is unsafe",
            Self::InvalidRepositoryPath => "repository path is invalid",
            Self::NotRepository => "target is not a supported repository",
            Self::GitUnavailable => "Git executable is unavailable",
            Self::GitTimedOut => "Git operation exceeded the local time limit",
            Self::GitFailed => "Git operation failed",
            Self::Io => "repository operation failed",
        };
        formatter.write_str(message)
    }
}

impl Error for RepositoryGitError {}

pub fn list_repositories(
    workspace_root: &Path,
) -> Result<Vec<RepositoryEntry>, RepositoryGitError> {
    let (canonical_workspace, repositories_root) = checked_repositories_root(workspace_root)?;

    let mut entries = fs::read_dir(&repositories_root)
        .map_err(|_| RepositoryGitError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RepositoryGitError::Io)?;

    entries.sort_by(|left, right| {
        left.file_name()
            .to_string_lossy()
            .to_lowercase()
            .cmp(&right.file_name().to_string_lossy().to_lowercase())
            .then_with(|| {
                left.file_name()
                    .to_string_lossy()
                    .cmp(&right.file_name().to_string_lossy())
            })
    });

    let mut repositories = Vec::new();

    for entry in entries {
        if repositories.len() >= MAX_REPOSITORIES {
            break;
        }

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| RepositoryGitError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }

        let canonical = fs::canonicalize(&path).map_err(|_| RepositoryGitError::Io)?;
        if !canonical.starts_with(&repositories_root)
            || !canonical.starts_with(&canonical_workspace)
        {
            return Err(RepositoryGitError::UnsafeRepositoriesRoot);
        }

        let git_dir = canonical.join(".git");
        let git_metadata = match fs::symlink_metadata(&git_dir) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
            continue;
        }

        repositories.push(RepositoryEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            relative_path: Path::new(REPOSITORIES_DIR).join(entry.file_name()),
        });
    }

    Ok(repositories)
}

pub fn repository_status(
    workspace_root: &Path,
    repository: &Path,
) -> Result<RepositoryStatus, RepositoryGitError> {
    let repo = resolve_repository(workspace_root, repository)?;
    let output = run_git(
        &repo,
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=normal",
            "--ignore-submodules=all",
            "--no-renames",
        ],
    )?;

    parse_status(&output.stdout, output.stdout_truncated)
}

pub fn recent_commits(
    workspace_root: &Path,
    repository: &Path,
) -> Result<GitTextOutput, RepositoryGitError> {
    let repo = resolve_repository(workspace_root, repository)?;
    let limit = format!("-n{RECENT_COMMIT_LIMIT}");
    let output = run_git(
        &repo,
        &[
            "log",
            &limit,
            "--no-decorate",
            "--no-notes",
            "--date=short",
            "--format=%h%x09%ad%x09%s",
        ],
    )?;

    Ok(GitTextOutput {
        text: sanitize_text(&output.stdout),
        truncated: output.stdout_truncated,
    })
}

pub fn worktree_diff_stat(
    workspace_root: &Path,
    repository: &Path,
) -> Result<GitTextOutput, RepositoryGitError> {
    diff_stat(workspace_root, repository, false)
}

pub fn staged_diff_stat(
    workspace_root: &Path,
    repository: &Path,
) -> Result<GitTextOutput, RepositoryGitError> {
    diff_stat(workspace_root, repository, true)
}

fn diff_stat(
    workspace_root: &Path,
    repository: &Path,
    staged: bool,
) -> Result<GitTextOutput, RepositoryGitError> {
    let repo = resolve_repository(workspace_root, repository)?;
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=all",
        "--no-renames",
        "--no-color",
        "--stat",
        "--stat-width=80",
        "--stat-name-width=48",
    ];
    if staged {
        args.insert(1, "--cached");
    }

    let output = run_git(&repo, &args)?;
    Ok(GitTextOutput {
        text: sanitize_text(&output.stdout),
        truncated: output.stdout_truncated,
    })
}

fn checked_repositories_root(
    workspace_root: &Path,
) -> Result<(PathBuf, PathBuf), RepositoryGitError> {
    let canonical_workspace = fs::canonicalize(workspace_root)
        .map_err(|_| RepositoryGitError::RepositoriesRootUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(RepositoryGitError::RepositoriesRootUnavailable);
    }

    let repositories = canonical_workspace.join(REPOSITORIES_DIR);
    let metadata = fs::symlink_metadata(&repositories)
        .map_err(|_| RepositoryGitError::RepositoriesRootUnavailable)?;
    if metadata.file_type().is_symlink() {
        return Err(RepositoryGitError::UnsafeRepositoriesRoot);
    }
    if !metadata.is_dir() {
        return Err(RepositoryGitError::RepositoriesRootUnavailable);
    }

    let canonical_repositories = fs::canonicalize(&repositories)
        .map_err(|_| RepositoryGitError::RepositoriesRootUnavailable)?;
    if !canonical_repositories.starts_with(&canonical_workspace) {
        return Err(RepositoryGitError::UnsafeRepositoriesRoot);
    }

    Ok((canonical_workspace, canonical_repositories))
}

fn resolve_repository(
    workspace_root: &Path,
    repository: &Path,
) -> Result<PathBuf, RepositoryGitError> {
    validate_repository_relative(repository)?;
    let (canonical_workspace, repositories_root) = checked_repositories_root(workspace_root)?;

    let candidate = canonical_workspace.join(repository);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|_| RepositoryGitError::NotRepository)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RepositoryGitError::NotRepository);
    }

    let canonical = fs::canonicalize(&candidate).map_err(|_| RepositoryGitError::NotRepository)?;
    if !canonical.starts_with(&repositories_root)
        || !canonical.starts_with(&canonical_workspace)
    {
        return Err(RepositoryGitError::InvalidRepositoryPath);
    }

    let git_dir = canonical.join(".git");
    let git_metadata =
        fs::symlink_metadata(&git_dir).map_err(|_| RepositoryGitError::NotRepository)?;
    if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
        return Err(RepositoryGitError::NotRepository);
    }

    Ok(canonical)
}

fn validate_repository_relative(repository: &Path) -> Result<(), RepositoryGitError> {
    if repository.is_absolute() {
        return Err(RepositoryGitError::InvalidRepositoryPath);
    }

    let mut components = repository.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Normal(root)), Some(Component::Normal(_name)), None)
            if root == OsStr::new(REPOSITORIES_DIR) =>
        {
            Ok(())
        }
        _ => Err(RepositoryGitError::InvalidRepositoryPath),
    }
}

fn parse_status(
    stdout: &[u8],
    output_truncated: bool,
) -> Result<RepositoryStatus, RepositoryGitError> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines();

    let branch = lines
        .next()
        .and_then(|line| line.strip_prefix("## "))
        .map(sanitize_line)
        .ok_or(RepositoryGitError::GitFailed)?;

    let mut result = RepositoryStatus {
        branch,
        entries: Vec::new(),
        staged: 0,
        unstaged: 0,
        untracked: 0,
        conflicted: 0,
        output_truncated,
        entry_limit_reached: false,
    };

    for line in lines {
        if line.len() < 3 {
            continue;
        }

        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let code = format!("{x}{y}");

        if code == "??" {
            result.untracked += 1;
        } else {
            if is_conflicted(&code) {
                result.conflicted += 1;
            }
            if x != ' ' {
                result.staged += 1;
            }
            if y != ' ' {
                result.unstaged += 1;
            }
        }

        if result.entries.len() < MAX_STATUS_ENTRIES {
            result.entries.push(RepositoryStatusEntry {
                code,
                path: sanitize_line(&line[3..]),
            });
        } else {
            result.entry_limit_reached = true;
        }
    }

    Ok(result)
}

fn is_conflicted(code: &str) -> bool {
    matches!(code, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
}

fn sanitize_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '�'
            }
        })
        .collect()
}

fn sanitize_line(value: &str) -> String {
    const MAX_LINE_CHARS: usize = 512;
    let mut output = String::new();
    let mut count = 0usize;

    for character in value.chars() {
        if count >= MAX_LINE_CHARS {
            output.push('…');
            break;
        }
        output.push(if character == '\t' || !character.is_control() {
            character
        } else {
            '�'
        });
        count += 1;
    }

    output
}

struct GitProcessOutput {
    stdout: Vec<u8>,
    stdout_truncated: bool,
}

fn run_git(repo: &Path, args: &[&str]) -> Result<GitProcessOutput, RepositoryGitError> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .arg("--no-replace-objects")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("submodule.recurse=false")
        .arg("-c")
        .arg("log.showSignature=false")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");

    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
        "GIT_PAGER",
        "GIT_EDITOR",
        "GIT_SEQUENCE_EDITOR",
        "GIT_ASKPASS",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_REDIRECT_STDIN",
        "GIT_REDIRECT_STDOUT",
        "GIT_REDIRECT_STDERR",
        "SSH_ASKPASS",
    ] {
        command.env_remove(variable);
    }

    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RepositoryGitError::GitUnavailable
        } else {
            RepositoryGitError::Io
        }
    })?;

    let stdout = child.stdout.take().ok_or(RepositoryGitError::Io)?;
    let stderr = child.stderr.take().ok_or(RepositoryGitError::Io)?;

    let stdout_reader = thread::spawn(move || drain_bounded(stdout));
    let stderr_reader = thread::spawn(move || drain_bounded(stderr));

    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|_| RepositoryGitError::Io)? {
            Some(status) => break status,
            None if started.elapsed() >= MAX_GIT_RUNTIME => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(RepositoryGitError::GitTimedOut);
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    };

    let (stdout, stdout_truncated) =
        stdout_reader.join().map_err(|_| RepositoryGitError::Io)?;
    let (_stderr, _stderr_truncated) =
        stderr_reader.join().map_err(|_| RepositoryGitError::Io)?;

    if !status.success() {
        return Err(RepositoryGitError::GitFailed);
    }

    Ok(GitProcessOutput {
        stdout,
        stdout_truncated,
    })
}

fn drain_bounded<R: Read>(mut reader: R) -> (Vec<u8>, bool) {
    let mut retained = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 16 * 1024];

    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };

        let remaining = MAX_GIT_OUTPUT_BYTES.saturating_sub(retained.len());
        if remaining > 0 {
            let keep = remaining.min(count);
            retained.extend_from_slice(&buffer[..keep]);
            if keep < count {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    (retained, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-repository-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("repositories")).expect("repositories");
        root
    }

    fn raw_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git executable");
        assert!(status.success(), "git test setup failed: {args:?}");
    }

    fn initialize_repo(root: &Path, name: &str) -> PathBuf {
        let repo = root.join("repositories").join(name);
        fs::create_dir_all(&repo).expect("repo directory");
        raw_git(&repo, &["init"]);
        repo
    }

    fn seed_commit(repo: &Path) {
        raw_git(repo, &["config", "user.name", "DANGOMUSHI Test"]);
        raw_git(repo, &["config", "user.email", "test@example.invalid"]);
        raw_git(repo, &["config", "core.autocrlf", "false"]);
        fs::write(repo.join("tracked.txt"), "first\n").expect("tracked");
        raw_git(repo, &["add", "tracked.txt"]);
        raw_git(repo, &["commit", "-m", "initial"]);
    }

    #[test]
    fn lists_only_direct_real_git_directories() {
        let root = temporary_workspace("list");
        initialize_repo(&root, "beta");
        initialize_repo(&root, "alpha");
        fs::create_dir_all(root.join("repositories/not-git")).expect("plain");

        let entries = list_repositories(&root).expect("list");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.relative_path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("repositories/alpha"),
                PathBuf::from("repositories/beta")
            ]
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn status_reports_modified_and_untracked_without_writing_sources() {
        let root = temporary_workspace("status");
        let repo = initialize_repo(&root, "demo");
        seed_commit(&repo);

        fs::write(repo.join("tracked.txt"), "changed\n").expect("modify");
        fs::write(repo.join("new.txt"), "new\n").expect("untracked");
        let tracked_before = fs::read(repo.join("tracked.txt")).expect("before tracked");
        let new_before = fs::read(repo.join("new.txt")).expect("before new");

        let status =
            repository_status(&root, Path::new("repositories/demo")).expect("status");
        assert_eq!(status.unstaged, 1);
        assert_eq!(status.untracked, 1);
        assert!(status.entries.iter().any(|entry| entry.code == " M"));
        assert!(status.entries.iter().any(|entry| entry.code == "??"));

        assert_eq!(
            fs::read(repo.join("tracked.txt")).expect("after tracked"),
            tracked_before
        );
        assert_eq!(fs::read(repo.join("new.txt")).expect("after new"), new_before);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn recent_commits_and_diff_stats_are_bounded_read_only_views() {
        let root = temporary_workspace("views");
        let repo = initialize_repo(&root, "demo");
        seed_commit(&repo);
        fs::write(repo.join("tracked.txt"), "first\nsecond\n").expect("modify");

        let log = recent_commits(&root, Path::new("repositories/demo")).expect("log");
        assert!(log.text.contains("initial"));
        assert!(!log.truncated);

        let diff =
            worktree_diff_stat(&root, Path::new("repositories/demo")).expect("diff");
        assert!(diff.text.contains("tracked.txt"));
        assert!(!diff.truncated);

        let staged =
            staged_diff_stat(&root, Path::new("repositories/demo")).expect("staged");
        assert!(staged.text.trim().is_empty());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_paths_outside_direct_repositories_children() {
        let root = temporary_workspace("path");
        initialize_repo(&root, "demo");

        for invalid in [
            Path::new("../demo"),
            Path::new("repositories"),
            Path::new("repositories/demo/nested"),
            Path::new("data/documents"),
        ] {
            assert_eq!(
                repository_status(&root, invalid).unwrap_err(),
                RepositoryGitError::InvalidRepositoryPath
            );
        }

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn bounded_drain_keeps_prefix_and_drains_remainder() {
        let payload = vec![b'x'; MAX_GIT_OUTPUT_BYTES + 1024];
        let (retained, truncated) = drain_bounded(payload.as_slice());
        assert_eq!(retained.len(), MAX_GIT_OUTPUT_BYTES);
        assert!(truncated);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_repository_and_git_directory() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink");
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).expect("outside");
        initialize_repo(&root, "real");
        symlink(&outside, root.join("repositories/link")).expect("repo link");

        let listed = list_repositories(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "real");

        let fake = root.join("repositories/fake");
        fs::create_dir_all(&fake).expect("fake");
        symlink(outside.join("git"), fake.join(".git")).expect("git link");
        assert_eq!(
            repository_status(&root, Path::new("repositories/fake")).unwrap_err(),
            RepositoryGitError::NotRepository
        );

        fs::remove_file(root.join("repositories/link")).expect("remove repo link");
        fs::remove_file(fake.join(".git")).expect("remove git link");
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
