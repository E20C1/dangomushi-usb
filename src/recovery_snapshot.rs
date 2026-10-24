use crate::workspace_backup::{self, WorkspaceBackupSummary};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const SNAPSHOT_MARKER: &str = ".dangomushi-recovery-snapshot";
pub const SNAPSHOT_FORMAT_VERSION: u16 = 1;
pub const MAX_SNAPSHOT_FILES: usize = 100_000;
pub const MAX_SNAPSHOT_PATH_BYTES: usize = 4 * 1024;
pub const MAX_SNAPSHOT_SINGLE_FILE_BYTES: u64 = 512 * 1024 * 1024 * 1024;
pub const MAX_SNAPSHOT_TOTAL_BYTES: u64 = 3 * 1024 * 1024 * 1024 * 1024;
pub const MAX_SNAPSHOT_MARKER_BYTES: usize = 32 * 1024 * 1024;

const SNAPSHOT_MAGIC: &str = "DANGOMUSHI_RECOVERY_SNAPSHOT_V1";
const SNAPSHOT_KIND: &str = "dangomushi-recovery-snapshot";
const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"DANGOMUSHI-RECOVERY-SNAPSHOT-DIGEST-V1\0";
const IO_BUFFER_BYTES: usize = 64 * 1024;
const RECOVERY_DIR: &str = "recovery";
const WORKSPACE_BACKUP_NAME: &str = "workspace.dgmwbk";

const SOURCE_FILES: &[&str] = &[
    ".gitignore",
    ".cargo/vendor-config.toml",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "README.md",
    "UPDATE_TOOLBOX.cmd",
];

const SOURCE_DIRS: &[&str] = &[
    ".github/workflows",
    "docs",
    "scripts",
    "src",
    "tests",
    "web",
    "vendor",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoverySnapshotSummary {
    pub files: usize,
    pub total_bytes: u64,
    pub sha256: [u8; 32],
    pub workspace_backup: WorkspaceBackupSummary,
}

impl RecoverySnapshotSummary {
    pub fn sha256_hex(&self) -> String {
        hex_lower(&self.sha256)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverySnapshotError {
    SourceUnavailable,
    VendorUnavailable,
    UnsafeSourceLayout,
    WorkspaceUnavailable,
    WorkspaceBackupUnavailable,
    UnsafeWorkspaceBackup,
    SnapshotUnavailable,
    UnsafeSnapshotLayout,
    InvalidSnapshotMarker,
    UnsupportedSnapshotVersion,
    DestinationExists,
    DestinationInsideInput,
    UnsafeDestinationParent,
    UnsupportedEntry,
    NonUtf8Path,
    PathLimitExceeded,
    EntryLimitExceeded,
    SizeLimitExceeded,
    SourceChanged,
    IntegrityFailed,
    Io,
}

impl fmt::Display for RecoverySnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SourceUnavailable => "recovery source tree is unavailable",
            Self::VendorUnavailable => "recovery source tree does not contain a usable vendor directory",
            Self::UnsafeSourceLayout => "recovery source tree layout is unsafe",
            Self::WorkspaceUnavailable => "recovery workspace is unavailable",
            Self::WorkspaceBackupUnavailable => "selected Workspace backup is unavailable or invalid",
            Self::UnsafeWorkspaceBackup => "selected Workspace backup path is unsafe",
            Self::SnapshotUnavailable => "recovery snapshot is unavailable",
            Self::UnsafeSnapshotLayout => "recovery snapshot layout is unsafe",
            Self::InvalidSnapshotMarker => "recovery snapshot marker is invalid",
            Self::UnsupportedSnapshotVersion => "recovery snapshot format version is unsupported",
            Self::DestinationExists => "recovery snapshot destination already exists",
            Self::DestinationInsideInput => {
                "recovery snapshot destination must be outside source and Workspace roots"
            }
            Self::UnsafeDestinationParent => "recovery snapshot destination parent is unsafe",
            Self::UnsupportedEntry => "recovery snapshot source contains an unsupported entry",
            Self::NonUtf8Path => "recovery snapshot requires UTF-8 path components",
            Self::PathLimitExceeded => "recovery snapshot path limit was exceeded",
            Self::EntryLimitExceeded => "recovery snapshot file-count limit was exceeded",
            Self::SizeLimitExceeded => "recovery snapshot size limit was exceeded",
            Self::SourceChanged => "recovery snapshot source changed while it was being copied",
            Self::IntegrityFailed => "recovery snapshot payload integrity check failed",
            Self::Io => "recovery snapshot I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for RecoverySnapshotError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileRecord {
    path: String,
    bytes: u64,
    sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSnapshotMarker {
    app_version: String,
    file_count: usize,
    total_bytes: u64,
    sha256: [u8; 32],
    records: Vec<FileRecord>,
}

struct SnapshotBuilder {
    records: Vec<FileRecord>,
    total_bytes: u64,
}

impl SnapshotBuilder {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            total_bytes: 0,
        }
    }

    fn push(&mut self, record: FileRecord) -> Result<(), RecoverySnapshotError> {
        if self.records.len() >= MAX_SNAPSHOT_FILES {
            return Err(RecoverySnapshotError::EntryLimitExceeded);
        }

        self.total_bytes = self
            .total_bytes
            .checked_add(record.bytes)
            .ok_or(RecoverySnapshotError::SizeLimitExceeded)?;
        if self.total_bytes > MAX_SNAPSHOT_TOTAL_BYTES {
            return Err(RecoverySnapshotError::SizeLimitExceeded);
        }

        self.records.push(record);
        Ok(())
    }

    fn finish(
        mut self,
        destination: &Path,
        workspace_backup: WorkspaceBackupSummary,
    ) -> Result<RecoverySnapshotSummary, RecoverySnapshotError> {
        self.records.sort_by(|left, right| left.path.cmp(&right.path));
        for pair in self.records.windows(2) {
            if pair[0].path == pair[1].path {
                return Err(RecoverySnapshotError::IntegrityFailed);
            }
        }

        verify_payload_exact(destination, &self.records)?;

        let digest = snapshot_digest(&self.records);
        write_marker(
            destination,
            self.records.len(),
            self.total_bytes,
            digest,
            &self.records,
        )?;

        sync_directory(destination)?;
        if let Some(parent) = destination.parent() {
            sync_directory(parent)?;
        }

        Ok(RecoverySnapshotSummary {
            files: self.records.len(),
            total_bytes: self.total_bytes,
            sha256: digest,
            workspace_backup,
        })
    }
}

pub fn create_snapshot(
    source_root: &Path,
    workspace_root: &Path,
    relative_workspace_backup: &Path,
    destination: &Path,
) -> Result<RecoverySnapshotSummary, RecoverySnapshotError> {
    let canonical_source = checked_source_root(source_root)?;
    let canonical_workspace = checked_workspace_root(workspace_root)?;
    let (canonical_destination_parent, destination_path) =
        checked_destination_path(destination, &canonical_source, &canonical_workspace)?;

    let workspace_summary = workspace_backup::verify_backup(
        &canonical_workspace,
        relative_workspace_backup,
    )
    .map_err(|_| RecoverySnapshotError::WorkspaceBackupUnavailable)?;

    let workspace_backup_path =
        resolve_verified_workspace_backup(&canonical_workspace, relative_workspace_backup)?;

    fs::create_dir(&destination_path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            RecoverySnapshotError::DestinationExists
        } else {
            RecoverySnapshotError::Io
        }
    })?;

    let result = populate_snapshot(
        &canonical_source,
        &workspace_backup_path,
        &destination_path,
        workspace_summary,
    );

    match result {
        Ok(summary) => Ok(summary),
        Err(error) => {
            let _ = fs::remove_dir_all(&destination_path);
            let _ = sync_directory(&canonical_destination_parent);
            Err(error)
        }
    }
}

pub fn verify_snapshot(
    snapshot_root: &Path,
) -> Result<RecoverySnapshotSummary, RecoverySnapshotError> {
    let metadata = fs::symlink_metadata(snapshot_root)
        .map_err(|_| RecoverySnapshotError::SnapshotUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
    }

    let canonical_root =
        fs::canonicalize(snapshot_root).map_err(|_| RecoverySnapshotError::SnapshotUnavailable)?;
    if !canonical_root.is_dir() {
        return Err(RecoverySnapshotError::SnapshotUnavailable);
    }

    let marker = read_snapshot_marker(&canonical_root)?;
    validate_snapshot_semantics(&canonical_root, &marker.records)?;
    verify_payload_exact(&canonical_root, &marker.records)?;

    if marker.file_count != marker.records.len() {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    let total_bytes = marker.records.iter().try_fold(0u64, |total, record| {
        total
            .checked_add(record.bytes)
            .ok_or(RecoverySnapshotError::SizeLimitExceeded)
    })?;
    if total_bytes != marker.total_bytes || total_bytes > MAX_SNAPSHOT_TOTAL_BYTES {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let digest = snapshot_digest_for_version(&marker.app_version, &marker.records);
    if digest != marker.sha256 {
        return Err(RecoverySnapshotError::IntegrityFailed);
    }

    let workspace_backup_path = canonical_root
        .join(RECOVERY_DIR)
        .join(WORKSPACE_BACKUP_NAME);
    let expected_backup = marker
        .records
        .iter()
        .find(|record| record.path == format!("{RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}"))
        .ok_or(RecoverySnapshotError::UnsafeSnapshotLayout)?;

    let workspace_backup = workspace_backup::verify_backup_file(&workspace_backup_path)
        .map_err(|_| RecoverySnapshotError::WorkspaceBackupUnavailable)?;

    let (backup_bytes, backup_sha256) = hash_file(&workspace_backup_path)?;
    if backup_bytes != expected_backup.bytes || backup_sha256 != expected_backup.sha256 {
        return Err(RecoverySnapshotError::IntegrityFailed);
    }

    Ok(RecoverySnapshotSummary {
        files: marker.file_count,
        total_bytes: marker.total_bytes,
        sha256: marker.sha256,
        workspace_backup,
    })
}

fn populate_snapshot(
    canonical_source: &Path,
    workspace_backup_path: &Path,
    destination: &Path,
    workspace_summary: WorkspaceBackupSummary,
) -> Result<RecoverySnapshotSummary, RecoverySnapshotError> {
    let mut builder = SnapshotBuilder::new();

    for relative in SOURCE_FILES {
        let source = canonical_source.join(relative);
        ensure_fixed_file(&source)?;
        let target = destination.join(relative);
        ensure_target_parent(destination, &target)?;
        builder.push(copy_regular_file(&source, &target, relative)?)?;
    }

    for relative in SOURCE_DIRS {
        let source = canonical_source.join(relative);
        if *relative == "vendor" {
            ensure_fixed_directory(&source)
                .map_err(|_| RecoverySnapshotError::VendorUnavailable)?;
        } else {
            ensure_fixed_directory(&source)?;
        }

        let target = destination.join(relative);
        fs::create_dir_all(&target).map_err(|_| RecoverySnapshotError::Io)?;
        copy_tree(
            canonical_source,
            &source,
            destination,
            relative,
            &mut builder,
        )?;
    }

    let recovery_dir = destination.join(RECOVERY_DIR);
    fs::create_dir(&recovery_dir).map_err(|_| RecoverySnapshotError::Io)?;
    let backup_relative = format!("{RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}");
    let backup_target = recovery_dir.join(WORKSPACE_BACKUP_NAME);
    builder.push(copy_regular_file(
        workspace_backup_path,
        &backup_target,
        &backup_relative,
    )?)?;

    builder.finish(destination, workspace_summary)
}

fn checked_source_root(path: &Path) -> Result<PathBuf, RecoverySnapshotError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| RecoverySnapshotError::SourceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RecoverySnapshotError::UnsafeSourceLayout);
    }

    let canonical =
        fs::canonicalize(path).map_err(|_| RecoverySnapshotError::SourceUnavailable)?;
    if !canonical.is_dir() {
        return Err(RecoverySnapshotError::SourceUnavailable);
    }

    for relative in SOURCE_FILES {
        ensure_fixed_file(&canonical.join(relative))?;
    }
    for relative in SOURCE_DIRS {
        let directory = canonical.join(relative);
        if *relative == "vendor" {
            ensure_fixed_directory(&directory)
                .map_err(|_| RecoverySnapshotError::VendorUnavailable)?;
        } else {
            ensure_fixed_directory(&directory)?;
        }
    }

    Ok(canonical)
}

fn checked_workspace_root(path: &Path) -> Result<PathBuf, RecoverySnapshotError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| RecoverySnapshotError::WorkspaceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RecoverySnapshotError::WorkspaceUnavailable);
    }

    fs::canonicalize(path).map_err(|_| RecoverySnapshotError::WorkspaceUnavailable)
}

fn checked_destination_path(
    destination: &Path,
    canonical_source: &Path,
    canonical_workspace: &Path,
) -> Result<(PathBuf, PathBuf), RecoverySnapshotError> {
    if destination.as_os_str().is_empty() {
        return Err(RecoverySnapshotError::UnsafeDestinationParent);
    }

    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(RecoverySnapshotError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(RecoverySnapshotError::Io),
    }

    let name = destination
        .file_name()
        .ok_or(RecoverySnapshotError::UnsafeDestinationParent)?;
    if name == OsStr::new(".") || name == OsStr::new("..") {
        return Err(RecoverySnapshotError::UnsafeDestinationParent);
    }

    let parent = match destination.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|_| RecoverySnapshotError::UnsafeDestinationParent)?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(RecoverySnapshotError::UnsafeDestinationParent);
    }

    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| RecoverySnapshotError::UnsafeDestinationParent)?;
    let final_path = canonical_parent.join(name);

    if final_path.starts_with(canonical_source) || final_path.starts_with(canonical_workspace) {
        return Err(RecoverySnapshotError::DestinationInsideInput);
    }

    Ok((canonical_parent, final_path))
}

fn ensure_fixed_file(path: &Path) -> Result<(), RecoverySnapshotError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| RecoverySnapshotError::SourceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RecoverySnapshotError::UnsafeSourceLayout);
    }
    Ok(())
}

fn ensure_fixed_directory(path: &Path) -> Result<(), RecoverySnapshotError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| RecoverySnapshotError::SourceUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RecoverySnapshotError::UnsafeSourceLayout);
    }
    Ok(())
}

fn copy_tree(
    canonical_source_root: &Path,
    source_dir: &Path,
    destination_root: &Path,
    relative_dir: &str,
    builder: &mut SnapshotBuilder,
) -> Result<(), RecoverySnapshotError> {
    let canonical_dir =
        fs::canonicalize(source_dir).map_err(|_| RecoverySnapshotError::SourceChanged)?;
    if !canonical_dir.starts_with(canonical_source_root) {
        return Err(RecoverySnapshotError::UnsafeSourceLayout);
    }

    let mut entries = fs::read_dir(source_dir)
        .map_err(|_| RecoverySnapshotError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RecoverySnapshotError::Io)?;

    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    for entry in entries {
        let name = entry
            .file_name()
            .to_str()
            .ok_or(RecoverySnapshotError::NonUtf8Path)?
            .to_string();
        validate_component(&name)?;

        let source = entry.path();
        let metadata =
            fs::symlink_metadata(&source).map_err(|_| RecoverySnapshotError::SourceChanged)?;
        if metadata.file_type().is_symlink() {
            return Err(RecoverySnapshotError::UnsupportedEntry);
        }

        let relative = format!("{relative_dir}/{name}");
        validate_relative_text(&relative)?;
        let target = destination_root.join(Path::new(&relative.replace('/', std::path::MAIN_SEPARATOR_STR)));

        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|_| RecoverySnapshotError::Io)?;
            copy_tree(
                canonical_source_root,
                &source,
                destination_root,
                &relative,
                builder,
            )?;
        } else if metadata.is_file() {
            builder.push(copy_regular_file(&source, &target, &relative)?)?;
        } else {
            return Err(RecoverySnapshotError::UnsupportedEntry);
        }
    }

    Ok(())
}

fn ensure_target_parent(root: &Path, target: &Path) -> Result<(), RecoverySnapshotError> {
    let parent = target.parent().ok_or(RecoverySnapshotError::Io)?;
    if parent == root {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|_| RecoverySnapshotError::Io)
}

fn copy_regular_file(
    source: &Path,
    destination: &Path,
    relative_text: &str,
) -> Result<FileRecord, RecoverySnapshotError> {
    validate_relative_text(relative_text)?;

    let before =
        fs::symlink_metadata(source).map_err(|_| RecoverySnapshotError::SourceChanged)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(RecoverySnapshotError::UnsupportedEntry);
    }
    if before.len() > MAX_SNAPSHOT_SINGLE_FILE_BYTES {
        return Err(RecoverySnapshotError::SizeLimitExceeded);
    }

    let modified_before = before.modified().ok();
    let mut input = File::open(source).map_err(|_| RecoverySnapshotError::Io)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut output = options
        .open(destination)
        .map_err(|_| RecoverySnapshotError::Io)?;

    let mut hasher = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0u8; IO_BUFFER_BYTES];

    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| RecoverySnapshotError::Io)?;
        if count == 0 {
            break;
        }

        copied = copied
            .checked_add(count as u64)
            .ok_or(RecoverySnapshotError::SizeLimitExceeded)?;
        if copied > MAX_SNAPSHOT_SINGLE_FILE_BYTES {
            return Err(RecoverySnapshotError::SizeLimitExceeded);
        }

        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| RecoverySnapshotError::Io)?;
    }

    output.sync_all().map_err(|_| RecoverySnapshotError::Io)?;
    drop(output);

    let after = fs::metadata(source).map_err(|_| RecoverySnapshotError::SourceChanged)?;
    if copied != before.len() || after.len() != before.len() {
        return Err(RecoverySnapshotError::SourceChanged);
    }
    if modified_before.is_some() && after.modified().ok() != modified_before {
        return Err(RecoverySnapshotError::SourceChanged);
    }

    let digest = hasher.finalize();
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(digest.as_slice());

    let (destination_bytes, destination_sha) = hash_file(destination)?;
    if destination_bytes != copied || destination_sha != sha256 {
        return Err(RecoverySnapshotError::IntegrityFailed);
    }

    Ok(FileRecord {
        path: relative_text.to_string(),
        bytes: copied,
        sha256,
    })
}

fn hash_file(path: &Path) -> Result<(u64, [u8; 32]), RecoverySnapshotError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RecoverySnapshotError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RecoverySnapshotError::IntegrityFailed);
    }

    let mut file = File::open(path).map_err(|_| RecoverySnapshotError::Io)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; IO_BUFFER_BYTES];

    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| RecoverySnapshotError::Io)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(RecoverySnapshotError::SizeLimitExceeded)?;
        hasher.update(&buffer[..count]);
    }

    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(digest.as_slice());
    Ok((total, output))
}

fn resolve_verified_workspace_backup(
    canonical_workspace: &Path,
    relative_backup: &Path,
) -> Result<PathBuf, RecoverySnapshotError> {
    if relative_backup.is_absolute() {
        return Err(RecoverySnapshotError::UnsafeWorkspaceBackup);
    }

    let mut components = relative_backup.components();
    let name = match (components.next(), components.next(), components.next()) {
        (
            Some(Component::Normal(root)),
            Some(Component::Normal(name)),
            None,
        ) if root == OsStr::new("backups")
            && Path::new(name).extension().and_then(OsStr::to_str) == Some("dgmwbk") =>
        {
            name
        }
        _ => return Err(RecoverySnapshotError::UnsafeWorkspaceBackup),
    };

    let backups = canonical_workspace.join("backups");
    let canonical_backups =
        fs::canonicalize(&backups).map_err(|_| RecoverySnapshotError::WorkspaceUnavailable)?;
    let candidate = canonical_backups.join(name);
    let metadata =
        fs::symlink_metadata(&candidate).map_err(|_| RecoverySnapshotError::WorkspaceBackupUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RecoverySnapshotError::UnsafeWorkspaceBackup);
    }

    let canonical =
        fs::canonicalize(&candidate).map_err(|_| RecoverySnapshotError::WorkspaceBackupUnavailable)?;
    if canonical.parent() != Some(canonical_backups.as_path()) {
        return Err(RecoverySnapshotError::UnsafeWorkspaceBackup);
    }

    Ok(canonical)
}

fn read_snapshot_marker(
    snapshot_root: &Path,
) -> Result<ParsedSnapshotMarker, RecoverySnapshotError> {
    let marker_path = snapshot_root.join(SNAPSHOT_MARKER);
    let metadata = fs::symlink_metadata(&marker_path)
        .map_err(|_| RecoverySnapshotError::InvalidSnapshotMarker)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    if metadata.len() > MAX_SNAPSHOT_MARKER_BYTES as u64 {
        return Err(RecoverySnapshotError::SizeLimitExceeded);
    }

    let mut file = File::open(&marker_path).map_err(|_| RecoverySnapshotError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take((MAX_SNAPSHOT_MARKER_BYTES as u64) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RecoverySnapshotError::Io)?;
    if bytes.len() > MAX_SNAPSHOT_MARKER_BYTES || bytes.len() as u64 != metadata.len() {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let text =
        String::from_utf8(bytes).map_err(|_| RecoverySnapshotError::InvalidSnapshotMarker)?;
    parse_snapshot_marker(&text)
}

fn parse_snapshot_marker(text: &str) -> Result<ParsedSnapshotMarker, RecoverySnapshotError> {
    if !text.ends_with('\n') {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() != Some(&"") {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    lines.pop();

    if lines.len() < 8 || lines[0] != SNAPSHOT_MAGIC {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    if lines[1] != format!("kind={SNAPSHOT_KIND}") {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let format_text = exact_prefixed(lines[2], "format=")?;
    let format = parse_canonical_u64(format_text)?;
    if format != SNAPSHOT_FORMAT_VERSION as u64 {
        return Err(RecoverySnapshotError::UnsupportedSnapshotVersion);
    }

    let app_version = exact_prefixed(lines[3], "app_version=")?;
    validate_app_version(app_version)?;

    if lines[4] != format!("workspace_backup={RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}") {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let file_count_u64 = parse_canonical_u64(exact_prefixed(lines[5], "file_count=")?)?;
    let file_count =
        usize::try_from(file_count_u64).map_err(|_| RecoverySnapshotError::EntryLimitExceeded)?;
    if file_count == 0 || file_count > MAX_SNAPSHOT_FILES {
        return Err(RecoverySnapshotError::EntryLimitExceeded);
    }

    let total_bytes = parse_canonical_u64(exact_prefixed(lines[6], "total_bytes=")?)?;
    if total_bytes > MAX_SNAPSHOT_TOTAL_BYTES {
        return Err(RecoverySnapshotError::SizeLimitExceeded);
    }

    let sha256 = parse_hex_32(exact_prefixed(lines[7], "snapshot_sha256=")?)?;

    if lines.len() != 8 + file_count {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let mut records = Vec::with_capacity(file_count);
    let mut previous_path: Option<String> = None;
    for line in &lines[8..] {
        let encoded = exact_prefixed(line, "entry=")?;
        let mut fields = encoded.split('|');
        let path_hex = fields.next().ok_or(RecoverySnapshotError::InvalidSnapshotMarker)?;
        let bytes_text = fields.next().ok_or(RecoverySnapshotError::InvalidSnapshotMarker)?;
        let sha_text = fields.next().ok_or(RecoverySnapshotError::InvalidSnapshotMarker)?;
        if fields.next().is_some() {
            return Err(RecoverySnapshotError::InvalidSnapshotMarker);
        }

        let path_bytes = decode_lower_hex(path_hex)?;
        if path_bytes.is_empty() || path_bytes.len() > MAX_SNAPSHOT_PATH_BYTES {
            return Err(RecoverySnapshotError::PathLimitExceeded);
        }
        let path =
            String::from_utf8(path_bytes).map_err(|_| RecoverySnapshotError::NonUtf8Path)?;
        validate_relative_text(&path)?;
        if hex_lower(path.as_bytes()) != path_hex {
            return Err(RecoverySnapshotError::InvalidSnapshotMarker);
        }

        if let Some(previous) = previous_path.as_deref() {
            if previous >= path.as_str() {
                return Err(RecoverySnapshotError::InvalidSnapshotMarker);
            }
        }

        let bytes = parse_canonical_u64(bytes_text)?;
        if bytes > MAX_SNAPSHOT_SINGLE_FILE_BYTES {
            return Err(RecoverySnapshotError::SizeLimitExceeded);
        }

        let record_sha = parse_hex_32(sha_text)?;
        previous_path = Some(path.clone());
        records.push(FileRecord {
            path,
            bytes,
            sha256: record_sha,
        });
    }

    Ok(ParsedSnapshotMarker {
        app_version: app_version.to_string(),
        file_count,
        total_bytes,
        sha256,
        records,
    })
}

fn exact_prefixed<'a>(
    line: &'a str,
    prefix: &str,
) -> Result<&'a str, RecoverySnapshotError> {
    line.strip_prefix(prefix)
        .filter(|value| !value.is_empty())
        .ok_or(RecoverySnapshotError::InvalidSnapshotMarker)
}

fn parse_canonical_u64(value: &str) -> Result<u64, RecoverySnapshotError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| RecoverySnapshotError::InvalidSnapshotMarker)?;
    if parsed.to_string() != value {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    Ok(parsed)
}

fn validate_app_version(value: &str) -> Result<(), RecoverySnapshotError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    Ok(())
}

fn decode_lower_hex(value: &str) -> Result<Vec<u8>, RecoverySnapshotError> {
    if value.len() % 2 != 0 {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    let mut output = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let high = lower_hex_nibble(pair[0])?;
        let low = lower_hex_nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn parse_hex_32(value: &str) -> Result<[u8; 32], RecoverySnapshotError> {
    if value.len() != 64 {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }
    let decoded = decode_lower_hex(value)?;
    decoded
        .try_into()
        .map_err(|_| RecoverySnapshotError::InvalidSnapshotMarker)
}

fn lower_hex_nibble(byte: u8) -> Result<u8, RecoverySnapshotError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(RecoverySnapshotError::InvalidSnapshotMarker),
    }
}

fn validate_snapshot_semantics(
    snapshot_root: &Path,
    records: &[FileRecord],
) -> Result<(), RecoverySnapshotError> {
    if records.is_empty() || records.len() > MAX_SNAPSHOT_FILES {
        return Err(RecoverySnapshotError::EntryLimitExceeded);
    }

    let paths = records
        .iter()
        .map(|record| record.path.as_str())
        .collect::<BTreeSet<_>>();
    if paths.len() != records.len() {
        return Err(RecoverySnapshotError::InvalidSnapshotMarker);
    }

    for required in SOURCE_FILES {
        if !paths.contains(required) {
            return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
        }
    }

    let backup_relative = format!("{RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}");
    if !paths.contains(backup_relative.as_str()) {
        return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
    }

    for directory in SOURCE_DIRS {
        let prefix = format!("{directory}/");
        if !records.iter().any(|record| record.path.starts_with(&prefix)) {
            return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
        }
    }

    for record in records {
        if !is_allowed_snapshot_file(&record.path) {
            return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
        }
    }

    for directory in [".cargo", ".github", RECOVERY_DIR] {
        ensure_snapshot_directory(snapshot_root, directory)?;
    }
    for directory in SOURCE_DIRS {
        ensure_snapshot_directory(snapshot_root, directory)?;
    }

    Ok(())
}

fn ensure_snapshot_directory(
    snapshot_root: &Path,
    relative: &str,
) -> Result<(), RecoverySnapshotError> {
    let path = join_relative(snapshot_root, relative)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|_| RecoverySnapshotError::UnsafeSnapshotLayout)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
    }
    Ok(())
}

fn is_allowed_snapshot_file(path: &str) -> bool {
    if SOURCE_FILES.contains(&path) {
        return true;
    }
    if path == format!("{RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}") {
        return true;
    }

    SOURCE_DIRS.iter().any(|directory| {
        let prefix = format!("{directory}/");
        path.starts_with(&prefix)
    })
}

fn is_allowed_snapshot_directory(path: &str) -> bool {
    if matches!(path, ".cargo" | ".github" | RECOVERY_DIR) {
        return true;
    }

    SOURCE_DIRS.iter().any(|directory| {
        path == *directory || path.starts_with(&format!("{directory}/"))
    })
}

fn verify_payload_exact(
    destination: &Path,
    expected: &[FileRecord],
) -> Result<(), RecoverySnapshotError> {
    let expected_paths = expected
        .iter()
        .map(|record| record.path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual_paths = BTreeSet::new();

    collect_payload_files(destination, destination, &mut actual_paths)?;

    if actual_paths != expected_paths {
        return Err(RecoverySnapshotError::IntegrityFailed);
    }

    for record in expected {
        let path = join_relative(destination, &record.path)?;
        let (bytes, sha256) = hash_file(&path)?;
        if bytes != record.bytes || sha256 != record.sha256 {
            return Err(RecoverySnapshotError::IntegrityFailed);
        }
    }

    Ok(())
}

fn collect_payload_files(
    root: &Path,
    directory: &Path,
    output: &mut BTreeSet<String>,
) -> Result<(), RecoverySnapshotError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|_| RecoverySnapshotError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| RecoverySnapshotError::Io)?;
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    for entry in entries {
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| RecoverySnapshotError::IntegrityFailed)?;
        if metadata.file_type().is_symlink() {
            return Err(RecoverySnapshotError::IntegrityFailed);
        }

        if metadata.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| RecoverySnapshotError::UnsafeSnapshotLayout)?;
            let text = path_to_forward_slash(relative)?;
            if !is_allowed_snapshot_directory(&text) {
                return Err(RecoverySnapshotError::UnsafeSnapshotLayout);
            }
            collect_payload_files(root, &path, output)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| RecoverySnapshotError::IntegrityFailed)?;
            let text = path_to_forward_slash(relative)?;
            if text == SNAPSHOT_MARKER {
                continue;
            }
            if !output.insert(text) {
                return Err(RecoverySnapshotError::IntegrityFailed);
            }
        } else {
            return Err(RecoverySnapshotError::IntegrityFailed);
        }
    }

    Ok(())
}

fn snapshot_digest(records: &[FileRecord]) -> [u8; 32] {
    snapshot_digest_for_version(env!("CARGO_PKG_VERSION"), records)
}

fn snapshot_digest_for_version(
    app_version: &str,
    records: &[FileRecord],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SNAPSHOT_DIGEST_DOMAIN);
    hasher.update(SNAPSHOT_FORMAT_VERSION.to_le_bytes());

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

    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(digest.as_slice());
    output
}

fn write_marker(
    destination: &Path,
    file_count: usize,
    total_bytes: u64,
    digest: [u8; 32],
    records: &[FileRecord],
) -> Result<(), RecoverySnapshotError> {
    let marker_path = destination.join(SNAPSHOT_MARKER);
    let mut marker = String::new();

    marker.push_str(SNAPSHOT_MAGIC);
    marker.push('\n');
    marker.push_str(&format!("kind={SNAPSHOT_KIND}\n"));
    marker.push_str(&format!("format={SNAPSHOT_FORMAT_VERSION}\n"));
    marker.push_str(&format!("app_version={}\n", env!("CARGO_PKG_VERSION")));
    marker.push_str(&format!(
        "workspace_backup={RECOVERY_DIR}/{WORKSPACE_BACKUP_NAME}\n"
    ));
    marker.push_str(&format!("file_count={file_count}\n"));
    marker.push_str(&format!("total_bytes={total_bytes}\n"));
    marker.push_str(&format!("snapshot_sha256={}\n", hex_lower(&digest)));

    for record in records {
        marker.push_str("entry=");
        marker.push_str(&hex_lower(record.path.as_bytes()));
        marker.push('|');
        marker.push_str(&record.bytes.to_string());
        marker.push('|');
        marker.push_str(&hex_lower(&record.sha256));
        marker.push('\n');

        if marker.len() > MAX_SNAPSHOT_MARKER_BYTES {
            return Err(RecoverySnapshotError::SizeLimitExceeded);
        }
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(marker_path)
        .map_err(|_| RecoverySnapshotError::Io)?;
    file.write_all(marker.as_bytes())
        .map_err(|_| RecoverySnapshotError::Io)?;
    file.sync_all().map_err(|_| RecoverySnapshotError::Io)?;
    Ok(())
}

fn join_relative(root: &Path, relative: &str) -> Result<PathBuf, RecoverySnapshotError> {
    validate_relative_text(relative)?;
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
    }
    Ok(path)
}

fn path_to_forward_slash(path: &Path) -> Result<String, RecoverySnapshotError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or(RecoverySnapshotError::NonUtf8Path)?;
                validate_component(value)?;
                parts.push(value.to_string());
            }
            _ => return Err(RecoverySnapshotError::PathLimitExceeded),
        }
    }

    let text = parts.join("/");
    validate_relative_text(&text)?;
    Ok(text)
}

fn validate_relative_text(path: &str) -> Result<(), RecoverySnapshotError> {
    if path.is_empty() || path.len() > MAX_SNAPSHOT_PATH_BYTES || path.starts_with('/') {
        return Err(RecoverySnapshotError::PathLimitExceeded);
    }

    for component in path.split('/') {
        validate_component(component)?;
    }

    Ok(())
}

fn validate_component(component: &str) -> Result<(), RecoverySnapshotError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
        || component.as_bytes().len() > 1024
    {
        return Err(RecoverySnapshotError::PathLimitExceeded);
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), RecoverySnapshotError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| RecoverySnapshotError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), RecoverySnapshotError> {
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-recovery-snapshot-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn fake_source(root: &Path) {
        fs::create_dir_all(root.join(".cargo")).expect("cargo dir");
        fs::create_dir_all(root.join(".github/workflows")).expect("workflow dir");
        for directory in ["docs", "scripts", "src", "tests", "web", "vendor/crate-1.0.0"] {
            fs::create_dir_all(root.join(directory)).expect("source dir");
        }

        for (path, bytes) in [
            (".gitignore", b"target/\n".as_slice()),
            (".cargo/vendor-config.toml", b"[net]\noffline = true\n".as_slice()),
            ("Cargo.toml", b"[package]\nname=\"fake\"\n".as_slice()),
            ("Cargo.lock", b"# generated\nversion = 4\n".as_slice()),
            ("rust-toolchain.toml", b"[toolchain]\nchannel=\"1.99.1\"\n".as_slice()),
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
            ("vendor/crate-1.0.0/src.rs", b"pub fn vendored() {}\n".as_slice()),
        ] {
            let target = root.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("parent");
            }
            fs::write(target, bytes).expect("source file");
        }
    }

    fn workspace_with_backup(root: &Path) -> PathBuf {
        workspace::initialize(root).expect("workspace");
        fs::write(root.join("data/documents/note.txt"), b"recovery-data")
            .expect("workspace data");
        let entry = workspace_backup::create_backup(root).expect("backup");
        entry.relative_path
    }

    #[test]
    fn create_snapshot_copies_build_inputs_vendor_and_verified_workspace_backup() {
        let source = temporary_path("source");
        let workspace_root = temporary_path("workspace");
        let destination_parent = temporary_path("destination-parent");
        fs::create_dir_all(&source).expect("source root");
        fs::create_dir_all(&destination_parent).expect("destination parent");
        fake_source(&source);
        let backup = workspace_with_backup(&workspace_root);

        fs::create_dir_all(source.join(".git")).expect("git");
        fs::write(source.join(".git/config"), b"must-not-copy").expect("git config");
        fs::create_dir_all(source.join("target")).expect("target");
        fs::write(source.join("target/generated.bin"), b"must-not-copy").expect("target file");
        fs::write(source.join("local-secret.env"), b"must-not-copy").expect("root secret");

        let destination = destination_parent.join("snapshot");
        let summary =
            create_snapshot(&source, &workspace_root, &backup, &destination).expect("snapshot");

        assert!(summary.files >= 10);
        assert!(summary.total_bytes > 0);
        assert!(destination.join(SNAPSHOT_MARKER).is_file());
        assert!(destination.join("Cargo.lock").is_file());
        assert!(destination.join("vendor/crate-1.0.0/src.rs").is_file());
        assert!(destination.join("recovery/workspace.dgmwbk").is_file());

        assert!(!destination.join(".git").exists());
        assert!(!destination.join("target").exists());
        assert!(!destination.join("local-secret.env").exists());
        assert!(!destination.join("workspace").exists());
        assert!(!destination.join("app").exists());

        let original_backup = fs::read(workspace_root.join(&backup)).expect("original backup");
        let copied_backup =
            fs::read(destination.join("recovery/workspace.dgmwbk")).expect("copied backup");
        assert_eq!(copied_backup, original_backup);

        let marker = fs::read_to_string(destination.join(SNAPSHOT_MARKER)).expect("marker");
        assert!(marker.starts_with(SNAPSHOT_MAGIC));
        assert!(marker.contains("workspace_backup=recovery/workspace.dgmwbk"));
        assert!(marker.contains(&format!("snapshot_sha256={}", summary.sha256_hex())));

        fs::remove_dir_all(source).expect("cleanup source");
        fs::remove_dir_all(workspace_root).expect("cleanup workspace");
        fs::remove_dir_all(destination_parent).expect("cleanup destination");
    }

    #[test]
    fn destination_must_be_create_new_and_outside_inputs() {
        let source = temporary_path("source-destination");
        let workspace_root = temporary_path("workspace-destination");
        let destination_parent = temporary_path("destination-parent-existing");
        fs::create_dir_all(&source).expect("source root");
        fs::create_dir_all(&destination_parent).expect("destination parent");
        fake_source(&source);
        let backup = workspace_with_backup(&workspace_root);

        let inside_source = source.join("snapshot");
        assert_eq!(
            create_snapshot(&source, &workspace_root, &backup, &inside_source).unwrap_err(),
            RecoverySnapshotError::DestinationInsideInput
        );

        let inside_workspace = workspace_root.join("snapshot");
        assert_eq!(
            create_snapshot(&source, &workspace_root, &backup, &inside_workspace).unwrap_err(),
            RecoverySnapshotError::DestinationInsideInput
        );

        let existing = destination_parent.join("existing");
        fs::create_dir(&existing).expect("existing");
        assert_eq!(
            create_snapshot(&source, &workspace_root, &backup, &existing).unwrap_err(),
            RecoverySnapshotError::DestinationExists
        );

        fs::remove_dir_all(source).expect("cleanup source");
        fs::remove_dir_all(workspace_root).expect("cleanup workspace");
        fs::remove_dir_all(destination_parent).expect("cleanup destination");
    }

    #[test]
    fn missing_vendor_fails_closed() {
        let source = temporary_path("source-no-vendor");
        let workspace_root = temporary_path("workspace-no-vendor");
        let destination_parent = temporary_path("destination-no-vendor");
        fs::create_dir_all(&source).expect("source root");
        fs::create_dir_all(&destination_parent).expect("destination parent");
        fake_source(&source);
        fs::remove_dir_all(source.join("vendor")).expect("remove vendor");
        let backup = workspace_with_backup(&workspace_root);

        let error = create_snapshot(
            &source,
            &workspace_root,
            &backup,
            &destination_parent.join("snapshot"),
        )
        .unwrap_err();
        assert_eq!(error, RecoverySnapshotError::VendorUnavailable);

        fs::remove_dir_all(source).expect("cleanup source");
        fs::remove_dir_all(workspace_root).expect("cleanup workspace");
        fs::remove_dir_all(destination_parent).expect("cleanup destination");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_source_tree_is_rejected() {
        use std::os::unix::fs::symlink;

        let source = temporary_path("source-symlink");
        let workspace_root = temporary_path("workspace-symlink");
        let destination_parent = temporary_path("destination-symlink");
        fs::create_dir_all(&source).expect("source root");
        fs::create_dir_all(&destination_parent).expect("destination parent");
        fake_source(&source);
        let outside = temporary_path("outside-symlink");
        fs::write(&outside, b"outside").expect("outside");
        symlink(&outside, source.join("docs/link.md")).expect("link");
        let backup = workspace_with_backup(&workspace_root);

        let error = create_snapshot(
            &source,
            &workspace_root,
            &backup,
            &destination_parent.join("snapshot"),
        )
        .unwrap_err();
        assert_eq!(error, RecoverySnapshotError::UnsupportedEntry);

        fs::remove_dir_all(source).expect("cleanup source");
        fs::remove_dir_all(workspace_root).expect("cleanup workspace");
        fs::remove_dir_all(destination_parent).expect("cleanup destination");
        fs::remove_file(outside).expect("cleanup outside");
    }
}
