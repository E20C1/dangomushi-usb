use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_MAGIC: &[u8; 8] = b"DGMWBK01";
const FOOTER_MAGIC: &[u8; 8] = b"DGMEND01";
const RECORD_END: u8 = 0;
const RECORD_DIRECTORY: u8 = 1;
const RECORD_FILE: u8 = 2;
const BACKUP_EXTENSION: &str = "dgmwbk";
const BACKUPS_DIR: &str = "backups";
const IO_BUFFER_BYTES: usize = 64 * 1024;

pub const MAX_BACKUPS_LISTED: usize = 512;
pub const MAX_BACKUP_ENTRIES: usize = 250_000;
pub const MAX_PATH_COMPONENTS: usize = 256;
pub const MAX_COMPONENT_BYTES: usize = 1024;
pub const MAX_SINGLE_FILE_BYTES: u64 = 512 * 1024 * 1024 * 1024;
pub const MAX_TOTAL_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;

const MANAGED_ROOTS: &[&[&str]] = &[
    &["data"],
    &["repositories"],
    &["identity"],
    &["config"],
    &["state", "autosave"],
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBackupEntry {
    pub name: String,
    pub relative_path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceBackupSummary {
    pub directories: usize,
    pub files: usize,
    pub total_file_bytes: u64,
    pub archive_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceBackupError {
    WorkspaceUnavailable,
    BackupsRootUnavailable,
    UnsafeWorkspaceLayout,
    UnsupportedEntry,
    NonUtf8Path,
    EntryLimitExceeded,
    PathLimitExceeded,
    BackupNameUnavailable,
    BackupNotFound,
    InvalidBackupPath,
    InvalidBackupFormat,
    BackupIntegrityFailed,
    SourceChanged,
    RestoreTargetUnsafe,
    RestoreFailed,
    Io,
}

impl fmt::Display for WorkspaceBackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WorkspaceUnavailable => "workspace is unavailable",
            Self::BackupsRootUnavailable => "workspace backups root is unavailable",
            Self::UnsafeWorkspaceLayout => "workspace backup layout is unsafe",
            Self::UnsupportedEntry => "workspace contains an unsupported backup entry",
            Self::NonUtf8Path => "workspace backup requires UTF-8 path components",
            Self::EntryLimitExceeded => "workspace backup entry limit was exceeded",
            Self::PathLimitExceeded => "workspace backup path limit was exceeded",
            Self::BackupNameUnavailable => "could not allocate a unique workspace backup name",
            Self::BackupNotFound => "workspace backup was not found",
            Self::InvalidBackupPath => "workspace backup path is invalid",
            Self::InvalidBackupFormat => "workspace backup format is invalid",
            Self::BackupIntegrityFailed => "workspace backup integrity verification failed",
            Self::SourceChanged => "workspace changed while backup was being created",
            Self::RestoreTargetUnsafe => "workspace restore target is not in a safe state",
            Self::RestoreFailed => "workspace restore failed and rollback was attempted",
            Self::Io => "workspace backup I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for WorkspaceBackupError {}

pub fn list_backups(
    workspace_root: &Path,
) -> Result<Vec<WorkspaceBackupEntry>, WorkspaceBackupError> {
    let (_, backups_root) = checked_workspace_and_backups(workspace_root)?;
    let mut entries = fs::read_dir(&backups_root)
        .map_err(|_| WorkspaceBackupError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| WorkspaceBackupError::Io)?;

    entries.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    let mut backups = Vec::new();
    for entry in entries {
        if backups.len() >= MAX_BACKUPS_LISTED {
            break;
        }

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| WorkspaceBackupError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if Path::new(&name).extension().and_then(OsStr::to_str) != Some(BACKUP_EXTENSION) {
            continue;
        }

        backups.push(WorkspaceBackupEntry {
            name: name.clone(),
            relative_path: Path::new(BACKUPS_DIR).join(name),
            bytes: metadata.len(),
        });
    }

    Ok(backups)
}

pub fn create_backup(
    workspace_root: &Path,
) -> Result<WorkspaceBackupEntry, WorkspaceBackupError> {
    let (canonical_workspace, backups_root) = checked_workspace_and_backups(workspace_root)?;
    let managed = resolve_managed_sources(&canonical_workspace)?;
    let (name, final_path, pending_path) = allocate_backup_paths(&backups_root)?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let pending_file = options
        .open(&pending_path)
        .map_err(|_| WorkspaceBackupError::Io)?;

    let write_result = write_archive(pending_file, &canonical_workspace, &managed);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&pending_path);
        return Err(error);
    }

    if let Err(error) = verify_archive_path(&pending_path, None) {
        let _ = fs::remove_file(&pending_path);
        return Err(error);
    }

    if backup_file_state(&final_path)? != BackupFileState::Missing {
        let _ = fs::remove_file(&pending_path);
        return Err(WorkspaceBackupError::BackupNameUnavailable);
    }

    if let Err(error) = copy_file_create_new(&pending_path, &final_path) {
        let _ = fs::remove_file(&final_path);
        let _ = fs::remove_file(&pending_path);
        return Err(error);
    }

    if let Err(error) = verify_archive_path(&final_path, None) {
        let _ = fs::remove_file(&final_path);
        let _ = fs::remove_file(&pending_path);
        return Err(error);
    }

    fs::remove_file(&pending_path).map_err(|_| WorkspaceBackupError::Io)?;
    sync_directory(&backups_root)?;

    let bytes = fs::metadata(&final_path)
        .map_err(|_| WorkspaceBackupError::Io)?
        .len();

    Ok(WorkspaceBackupEntry {
        name: name.clone(),
        relative_path: Path::new(BACKUPS_DIR).join(name),
        bytes,
    })
}

pub fn verify_backup(
    workspace_root: &Path,
    relative_backup: &Path,
) -> Result<WorkspaceBackupSummary, WorkspaceBackupError> {
    let (_, backups_root) = checked_workspace_and_backups(workspace_root)?;
    let backup_path = resolve_backup_file(&backups_root, relative_backup)?;
    verify_archive_path(&backup_path, None)
}

pub fn restore_backup(
    workspace_root: &Path,
    relative_backup: &Path,
) -> Result<WorkspaceBackupSummary, WorkspaceBackupError> {
    let (canonical_workspace, backups_root) = checked_workspace_and_backups(workspace_root)?;
    ensure_no_restore_artifacts(&backups_root)?;
    let backup_path = resolve_backup_file(&backups_root, relative_backup)?;

    let (staging_root, rollback_root) = allocate_restore_dirs(&backups_root)?;
    if let Err(error) = fs::create_dir(&staging_root) {
        return Err(map_io(error));
    }
    if let Err(error) = fs::create_dir(&rollback_root) {
        let _ = fs::remove_dir_all(&staging_root);
        return Err(map_io(error));
    }

    let summary = match verify_archive_path(&backup_path, Some(&staging_root)) {
        Ok(summary) => summary,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_root);
            let _ = fs::remove_dir_all(&rollback_root);
            return Err(error);
        }
    };

    if let Err(error) = validate_restore_targets(&canonical_workspace, &staging_root) {
        let _ = fs::remove_dir_all(&staging_root);
        let _ = fs::remove_dir_all(&rollback_root);
        return Err(error);
    }

    if let Err(error) = replace_managed_roots(
        &canonical_workspace,
        &staging_root,
        &rollback_root,
        &backups_root,
    ) {
        let _ = fs::remove_dir_all(&staging_root);
        let _ = fs::remove_dir_all(&rollback_root);
        return Err(error);
    }

    let _ = fs::remove_dir_all(&staging_root);
    let _ = fs::remove_dir_all(&rollback_root);
    let _ = sync_directory(&backups_root);
    let _ = sync_directory(&canonical_workspace);

    Ok(summary)
}

fn checked_workspace_and_backups(
    workspace_root: &Path,
) -> Result<(PathBuf, PathBuf), WorkspaceBackupError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| WorkspaceBackupError::WorkspaceUnavailable)?;
    if !canonical_workspace.is_dir() {
        return Err(WorkspaceBackupError::WorkspaceUnavailable);
    }

    let backups = canonical_workspace.join(BACKUPS_DIR);
    let metadata = fs::symlink_metadata(&backups)
        .map_err(|_| WorkspaceBackupError::BackupsRootUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
    }

    let canonical_backups =
        fs::canonicalize(&backups).map_err(|_| WorkspaceBackupError::BackupsRootUnavailable)?;
    if !canonical_backups.starts_with(&canonical_workspace) {
        return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
    }

    Ok((canonical_workspace, canonical_backups))
}

#[derive(Debug)]
struct ManagedSource {
    components: Vec<String>,
    source: Option<PathBuf>,
}

fn resolve_managed_sources(
    canonical_workspace: &Path,
) -> Result<Vec<ManagedSource>, WorkspaceBackupError> {
    let mut managed = Vec::new();

    for root in MANAGED_ROOTS {
        let components = root.iter().map(|part| (*part).to_string()).collect::<Vec<_>>();
        let mut cursor = canonical_workspace.to_path_buf();
        let mut missing_optional = false;

        for (index, component) in root.iter().enumerate() {
            cursor.push(component);
            match fs::symlink_metadata(&cursor) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && root.len() == 2
                        && root[0] == "state"
                        && root[1] == "autosave"
                        && index == root.len() - 1 =>
                {
                    missing_optional = true;
                }
                Err(_) => return Err(WorkspaceBackupError::UnsafeWorkspaceLayout),
            }

            if missing_optional {
                break;
            }
        }

        let source = if missing_optional {
            None
        } else {
            let canonical =
                fs::canonicalize(&cursor).map_err(|_| WorkspaceBackupError::UnsafeWorkspaceLayout)?;
            if !canonical.starts_with(canonical_workspace) {
                return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
            }
            Some(canonical)
        };

        managed.push(ManagedSource { components, source });
    }

    Ok(managed)
}

struct ArchiveWriter {
    file: File,
    archive_hasher: Sha256,
    bytes_written: u64,
}

impl ArchiveWriter {
    fn new(file: File) -> Self {
        Self {
            file,
            archive_hasher: Sha256::new(),
            bytes_written: 0,
        }
    }

    fn hashed(&mut self, bytes: &[u8]) -> Result<(), WorkspaceBackupError> {
        self.file
            .write_all(bytes)
            .map_err(|_| WorkspaceBackupError::Io)?;
        self.archive_hasher.update(bytes);
        self.bytes_written = self
            .bytes_written
            .checked_add(bytes.len() as u64)
            .ok_or(WorkspaceBackupError::InvalidBackupFormat)?;
        Ok(())
    }

    fn raw(&mut self, bytes: &[u8]) -> Result<(), WorkspaceBackupError> {
        self.file
            .write_all(bytes)
            .map_err(|_| WorkspaceBackupError::Io)?;
        self.bytes_written = self
            .bytes_written
            .checked_add(bytes.len() as u64)
            .ok_or(WorkspaceBackupError::InvalidBackupFormat)?;
        Ok(())
    }

    fn finish(mut self) -> Result<u64, WorkspaceBackupError> {
        let digest = self.archive_hasher.clone().finalize();
        self.raw(FOOTER_MAGIC)?;
        self.raw(digest.as_slice())?;
        self.file.sync_all().map_err(|_| WorkspaceBackupError::Io)?;
        Ok(self.bytes_written)
    }
}

fn write_archive(
    file: File,
    canonical_workspace: &Path,
    managed: &[ManagedSource],
) -> Result<WorkspaceBackupSummary, WorkspaceBackupError> {
    let mut writer = ArchiveWriter::new(file);
    writer.hashed(BACKUP_MAGIC)?;

    let mut summary = WorkspaceBackupSummary {
        directories: 0,
        files: 0,
        total_file_bytes: 0,
        archive_bytes: 0,
    };
    let mut entries = 0usize;

    for source in managed {
        write_directory_record(&mut writer, &source.components, &mut entries, &mut summary)?;
        if let Some(source_path) = &source.source {
            write_directory_tree(
                &mut writer,
                canonical_workspace,
                source_path,
                &source.components,
                &mut entries,
                &mut summary,
            )?;
        }
    }

    writer.hashed(&[RECORD_END])?;
    summary.archive_bytes = writer.finish()?;
    Ok(summary)
}

fn write_directory_tree(
    writer: &mut ArchiveWriter,
    canonical_workspace: &Path,
    source_dir: &Path,
    components: &[String],
    entries: &mut usize,
    summary: &mut WorkspaceBackupSummary,
) -> Result<(), WorkspaceBackupError> {
    let mut directory_entries = fs::read_dir(source_dir)
        .map_err(|_| WorkspaceBackupError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| WorkspaceBackupError::Io)?;

    directory_entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    for entry in directory_entries {
        let name = entry
            .file_name()
            .to_str()
            .ok_or(WorkspaceBackupError::NonUtf8Path)?
            .to_string();
        validate_component_text(&name)?;

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| WorkspaceBackupError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(WorkspaceBackupError::UnsupportedEntry);
        }

        let canonical =
            fs::canonicalize(&path).map_err(|_| WorkspaceBackupError::SourceChanged)?;
        if !canonical.starts_with(canonical_workspace) {
            return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
        }

        let mut child_components = components.to_vec();
        child_components.push(name);

        if metadata.is_dir() {
            write_directory_record(writer, &child_components, entries, summary)?;
            write_directory_tree(
                writer,
                canonical_workspace,
                &canonical,
                &child_components,
                entries,
                summary,
            )?;
        } else if metadata.is_file() {
            write_file_record(writer, &canonical, &child_components, entries, summary)?;
        } else {
            return Err(WorkspaceBackupError::UnsupportedEntry);
        }
    }

    Ok(())
}

fn write_directory_record(
    writer: &mut ArchiveWriter,
    components: &[String],
    entries: &mut usize,
    summary: &mut WorkspaceBackupSummary,
) -> Result<(), WorkspaceBackupError> {
    count_entry(entries)?;
    writer.hashed(&[RECORD_DIRECTORY])?;
    write_components(writer, components)?;
    summary.directories = summary.directories.saturating_add(1);
    Ok(())
}

fn write_file_record(
    writer: &mut ArchiveWriter,
    path: &Path,
    components: &[String],
    entries: &mut usize,
    summary: &mut WorkspaceBackupSummary,
) -> Result<(), WorkspaceBackupError> {
    count_entry(entries)?;

    let mut file = File::open(path).map_err(|_| WorkspaceBackupError::Io)?;
    let before = file.metadata().map_err(|_| WorkspaceBackupError::Io)?;
    if !before.is_file() {
        return Err(WorkspaceBackupError::SourceChanged);
    }
    if before.len() > MAX_SINGLE_FILE_BYTES {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }

    summary.total_file_bytes = summary
        .total_file_bytes
        .checked_add(before.len())
        .ok_or(WorkspaceBackupError::InvalidBackupFormat)?;
    if summary.total_file_bytes > MAX_TOTAL_FILE_BYTES {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }

    writer.hashed(&[RECORD_FILE])?;
    write_components(writer, components)?;
    writer.hashed(&before.len().to_le_bytes())?;

    let mut file_hasher = Sha256::new();
    let mut remaining = before.len();
    let mut buffer = [0u8; IO_BUFFER_BYTES];

    while remaining > 0 {
        let wanted = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| WorkspaceBackupError::Io)?;
        let count = file
            .read(&mut buffer[..wanted])
            .map_err(|_| WorkspaceBackupError::Io)?;
        if count == 0 {
            return Err(WorkspaceBackupError::SourceChanged);
        }

        writer.hashed(&buffer[..count])?;
        file_hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }

    let mut extra = [0u8; 1];
    if file.read(&mut extra).map_err(|_| WorkspaceBackupError::Io)? != 0 {
        return Err(WorkspaceBackupError::SourceChanged);
    }

    let after = file.metadata().map_err(|_| WorkspaceBackupError::Io)?;
    if after.len() != before.len()
        || (before.modified().ok().is_some()
            && after.modified().ok().is_some()
            && before.modified().ok() != after.modified().ok())
    {
        return Err(WorkspaceBackupError::SourceChanged);
    }

    let digest = file_hasher.finalize();
    writer.hashed(digest.as_slice())?;
    summary.files = summary.files.saturating_add(1);
    Ok(())
}

fn count_entry(entries: &mut usize) -> Result<(), WorkspaceBackupError> {
    if *entries >= MAX_BACKUP_ENTRIES {
        return Err(WorkspaceBackupError::EntryLimitExceeded);
    }
    *entries += 1;
    Ok(())
}

fn write_components(
    writer: &mut ArchiveWriter,
    components: &[String],
) -> Result<(), WorkspaceBackupError> {
    if components.is_empty() || components.len() > MAX_PATH_COMPONENTS {
        return Err(WorkspaceBackupError::PathLimitExceeded);
    }

    let count =
        u16::try_from(components.len()).map_err(|_| WorkspaceBackupError::PathLimitExceeded)?;
    writer.hashed(&count.to_le_bytes())?;

    for component in components {
        validate_component_text(component)?;
        let bytes = component.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_COMPONENT_BYTES {
            return Err(WorkspaceBackupError::PathLimitExceeded);
        }
        let length =
            u16::try_from(bytes.len()).map_err(|_| WorkspaceBackupError::PathLimitExceeded)?;
        writer.hashed(&length.to_le_bytes())?;
        writer.hashed(bytes)?;
    }

    Ok(())
}

fn validate_component_text(component: &str) -> Result<(), WorkspaceBackupError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
    {
        return Err(WorkspaceBackupError::PathLimitExceeded);
    }

    let mut parsed = Path::new(component).components();
    if !matches!(parsed.next(), Some(Component::Normal(_))) || parsed.next().is_some() {
        return Err(WorkspaceBackupError::PathLimitExceeded);
    }

    Ok(())
}

struct HashingReader {
    file: File,
    archive_hasher: Sha256,
    bytes_hashed: u64,
}

impl HashingReader {
    fn new(file: File) -> Self {
        Self {
            file,
            archive_hasher: Sha256::new(),
            bytes_hashed: 0,
        }
    }

    fn read_hashed(&mut self, bytes: &mut [u8]) -> Result<(), WorkspaceBackupError> {
        self.file
            .read_exact(bytes)
            .map_err(|_| WorkspaceBackupError::InvalidBackupFormat)?;
        let count = bytes.len() as u64;
        self.archive_hasher.update(&*bytes);
        self.bytes_hashed = self
            .bytes_hashed
            .checked_add(count)
            .ok_or(WorkspaceBackupError::InvalidBackupFormat)?;
        Ok(())
    }

    fn read_raw(&mut self, bytes: &mut [u8]) -> Result<(), WorkspaceBackupError> {
        self.file
            .read_exact(bytes)
            .map_err(|_| WorkspaceBackupError::InvalidBackupFormat)
    }

    fn finish_hash(&self) -> [u8; 32] {
        let digest = self.archive_hasher.clone().finalize();
        let mut output = [0u8; 32];
        output.copy_from_slice(digest.as_slice());
        output
    }
}

fn verify_archive_path(
    backup_path: &Path,
    extraction_root: Option<&Path>,
) -> Result<WorkspaceBackupSummary, WorkspaceBackupError> {
    let metadata = fs::symlink_metadata(backup_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            WorkspaceBackupError::BackupNotFound
        } else {
            WorkspaceBackupError::Io
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(WorkspaceBackupError::InvalidBackupPath);
    }

    let file = File::open(backup_path).map_err(|_| WorkspaceBackupError::Io)?;
    let mut reader = HashingReader::new(file);

    let mut magic = [0u8; 8];
    reader.read_hashed(&mut magic)?;
    if &magic != BACKUP_MAGIC {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }

    let mut seen = BTreeSet::<Vec<String>>::new();
    let mut directories = BTreeSet::<Vec<String>>::new();
    let mut roots_seen = BTreeSet::<Vec<String>>::new();
    let mut summary = WorkspaceBackupSummary {
        directories: 0,
        files: 0,
        total_file_bytes: 0,
        archive_bytes: metadata.len(),
    };
    let mut entry_count = 0usize;

    loop {
        let mut tag = [0u8; 1];
        reader.read_hashed(&mut tag)?;
        if tag[0] == RECORD_END {
            break;
        }
        if tag[0] != RECORD_DIRECTORY && tag[0] != RECORD_FILE {
            return Err(WorkspaceBackupError::InvalidBackupFormat);
        }

        entry_count = entry_count
            .checked_add(1)
            .ok_or(WorkspaceBackupError::EntryLimitExceeded)?;
        if entry_count > MAX_BACKUP_ENTRIES {
            return Err(WorkspaceBackupError::EntryLimitExceeded);
        }

        let components = read_components(&mut reader)?;
        if !is_managed_path(&components) || !seen.insert(components.clone()) {
            return Err(WorkspaceBackupError::InvalidBackupFormat);
        }

        if is_managed_root(&components) {
            if tag[0] != RECORD_DIRECTORY {
                return Err(WorkspaceBackupError::InvalidBackupFormat);
            }
            roots_seen.insert(components.clone());
        } else {
            let parent = components[..components.len() - 1].to_vec();
            if !directories.contains(&parent) {
                return Err(WorkspaceBackupError::InvalidBackupFormat);
            }
        }

        if tag[0] == RECORD_DIRECTORY {
            if let Some(root) = extraction_root {
                let target = join_components(root, &components)?;
                if let Some(parent) = target.parent() {
                    if parent != root && !parent.exists() {
                        fs::create_dir_all(parent).map_err(|_| WorkspaceBackupError::Io)?;
                    }
                }
                fs::create_dir(&target).map_err(|_| WorkspaceBackupError::Io)?;
            }
            directories.insert(components);
            summary.directories = summary.directories.saturating_add(1);
            continue;
        }

        let length = read_u64_hashed(&mut reader)?;
        if length > MAX_SINGLE_FILE_BYTES {
            return Err(WorkspaceBackupError::InvalidBackupFormat);
        }
        summary.total_file_bytes = summary
            .total_file_bytes
            .checked_add(length)
            .ok_or(WorkspaceBackupError::InvalidBackupFormat)?;
        if summary.total_file_bytes > MAX_TOTAL_FILE_BYTES {
            return Err(WorkspaceBackupError::InvalidBackupFormat);
        }

        let target = extraction_root
            .map(|root| join_components(root, &components))
            .transpose()?;
        let mut output = match target {
            Some(path) => {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                Some(options.open(path).map_err(|_| WorkspaceBackupError::Io)?)
            }
            None => None,
        };

        let mut file_hasher = Sha256::new();
        let mut remaining = length;
        let mut buffer = [0u8; IO_BUFFER_BYTES];

        while remaining > 0 {
            let count = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| WorkspaceBackupError::InvalidBackupFormat)?;
            reader.read_hashed(&mut buffer[..count])?;
            file_hasher.update(&buffer[..count]);
            if let Some(file) = output.as_mut() {
                file.write_all(&buffer[..count])
                    .map_err(|_| WorkspaceBackupError::Io)?;
            }
            remaining -= count as u64;
        }

        let mut expected = [0u8; 32];
        reader.read_hashed(&mut expected)?;
        let actual = file_hasher.finalize();
        if actual.as_slice() != expected {
            return Err(WorkspaceBackupError::BackupIntegrityFailed);
        }

        if let Some(file) = output {
            file.sync_all().map_err(|_| WorkspaceBackupError::Io)?;
        }

        summary.files = summary.files.saturating_add(1);
    }

    for root in MANAGED_ROOTS {
        let expected = root.iter().map(|part| (*part).to_string()).collect::<Vec<_>>();
        if !roots_seen.contains(&expected) {
            return Err(WorkspaceBackupError::InvalidBackupFormat);
        }
    }

    let computed = reader.finish_hash();

    let mut footer_magic = [0u8; 8];
    reader.read_raw(&mut footer_magic)?;
    if &footer_magic != FOOTER_MAGIC {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }

    let mut expected_archive = [0u8; 32];
    reader.read_raw(&mut expected_archive)?;
    if computed != expected_archive {
        return Err(WorkspaceBackupError::BackupIntegrityFailed);
    }

    let mut trailing = [0u8; 1];
    if reader
        .file
        .read(&mut trailing)
        .map_err(|_| WorkspaceBackupError::Io)?
        != 0
    {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }

    Ok(summary)
}

fn read_components(reader: &mut HashingReader) -> Result<Vec<String>, WorkspaceBackupError> {
    let count = read_u16_hashed(reader)? as usize;
    if count == 0 || count > MAX_PATH_COMPONENTS {
        return Err(WorkspaceBackupError::PathLimitExceeded);
    }

    let mut components = Vec::with_capacity(count);
    for _ in 0..count {
        let length = read_u16_hashed(reader)? as usize;
        if length == 0 || length > MAX_COMPONENT_BYTES {
            return Err(WorkspaceBackupError::PathLimitExceeded);
        }

        let mut bytes = vec![0u8; length];
        reader.read_hashed(&mut bytes)?;
        let component =
            String::from_utf8(bytes).map_err(|_| WorkspaceBackupError::NonUtf8Path)?;
        validate_component_text(&component)?;
        components.push(component);
    }

    Ok(components)
}

fn read_u16_hashed(reader: &mut HashingReader) -> Result<u16, WorkspaceBackupError> {
    let mut bytes = [0u8; 2];
    reader.read_hashed(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u64_hashed(reader: &mut HashingReader) -> Result<u64, WorkspaceBackupError> {
    let mut bytes = [0u8; 8];
    reader.read_hashed(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn is_managed_root(components: &[String]) -> bool {
    MANAGED_ROOTS.iter().any(|root| {
        components.len() == root.len()
            && components
                .iter()
                .map(String::as_str)
                .eq(root.iter().copied())
    })
}

fn is_managed_path(components: &[String]) -> bool {
    MANAGED_ROOTS.iter().any(|root| {
        components.len() >= root.len()
            && components
                .iter()
                .take(root.len())
                .map(String::as_str)
                .eq(root.iter().copied())
    })
}

fn join_components(root: &Path, components: &[String]) -> Result<PathBuf, WorkspaceBackupError> {
    let mut path = root.to_path_buf();
    for component in components {
        validate_component_text(component)?;
        path.push(component);
    }
    if !path.starts_with(root) {
        return Err(WorkspaceBackupError::InvalidBackupFormat);
    }
    Ok(path)
}

fn allocate_backup_paths(
    backups_root: &Path,
) -> Result<(String, PathBuf, PathBuf), WorkspaceBackupError> {
    let millis = timestamp_millis();

    for counter in 0..1000u16 {
        let name = format!("workspace-{millis:013}-{counter:03}.{BACKUP_EXTENSION}");
        let final_path = backups_root.join(&name);
        let pending_path = backups_root.join(format!(".{name}.pending"));

        if backup_file_state(&final_path)? == BackupFileState::Missing
            && backup_file_state(&pending_path)? == BackupFileState::Missing
        {
            return Ok((name, final_path, pending_path));
        }
    }

    Err(WorkspaceBackupError::BackupNameUnavailable)
}

fn allocate_restore_dirs(
    backups_root: &Path,
) -> Result<(PathBuf, PathBuf), WorkspaceBackupError> {
    let millis = timestamp_millis();

    for counter in 0..1000u16 {
        let staging = backups_root.join(format!(".restore-staging-{millis:013}-{counter:03}"));
        let rollback = backups_root.join(format!(".restore-rollback-{millis:013}-{counter:03}"));
        if !path_exists(&staging)? && !path_exists(&rollback)? {
            return Ok((staging, rollback));
        }
    }

    Err(WorkspaceBackupError::RestoreTargetUnsafe)
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackupFileState {
    Missing,
    Regular,
}

fn backup_file_state(path: &Path) -> Result<BackupFileState, WorkspaceBackupError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(WorkspaceBackupError::UnsafeWorkspaceLayout);
            }
            Ok(BackupFileState::Regular)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BackupFileState::Missing),
        Err(_) => Err(WorkspaceBackupError::Io),
    }
}

fn path_exists(path: &Path) -> Result<bool, WorkspaceBackupError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(WorkspaceBackupError::Io),
    }
}

fn resolve_backup_file(
    backups_root: &Path,
    relative_backup: &Path,
) -> Result<PathBuf, WorkspaceBackupError> {
    validate_backup_relative(relative_backup)?;
    let file_name = relative_backup
        .file_name()
        .ok_or(WorkspaceBackupError::InvalidBackupPath)?;
    let candidate = backups_root.join(file_name);

    match backup_file_state(&candidate)? {
        BackupFileState::Missing => return Err(WorkspaceBackupError::BackupNotFound),
        BackupFileState::Regular => {}
    }

    let canonical = fs::canonicalize(&candidate).map_err(|_| WorkspaceBackupError::BackupNotFound)?;
    if canonical.parent() != Some(backups_root) {
        return Err(WorkspaceBackupError::InvalidBackupPath);
    }

    Ok(canonical)
}

fn validate_backup_relative(relative_backup: &Path) -> Result<(), WorkspaceBackupError> {
    if relative_backup.is_absolute() {
        return Err(WorkspaceBackupError::InvalidBackupPath);
    }

    let mut components = relative_backup.components();
    match (components.next(), components.next(), components.next()) {
        (
            Some(Component::Normal(root)),
            Some(Component::Normal(name)),
            None,
        ) if root == OsStr::new(BACKUPS_DIR)
            && Path::new(name)
                .extension()
                .and_then(OsStr::to_str)
                == Some(BACKUP_EXTENSION) =>
        {
            Ok(())
        }
        _ => Err(WorkspaceBackupError::InvalidBackupPath),
    }
}

fn copy_file_create_new(source: &Path, destination: &Path) -> Result<(), WorkspaceBackupError> {
    let mut input = File::open(source).map_err(|_| WorkspaceBackupError::Io)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut output = options
        .open(destination)
        .map_err(|_| WorkspaceBackupError::Io)?;

    let mut buffer = [0u8; IO_BUFFER_BYTES];
    loop {
        let count = input.read(&mut buffer).map_err(|_| WorkspaceBackupError::Io)?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| WorkspaceBackupError::Io)?;
    }
    output.sync_all().map_err(|_| WorkspaceBackupError::Io)?;
    Ok(())
}

fn ensure_no_restore_artifacts(backups_root: &Path) -> Result<(), WorkspaceBackupError> {
    for entry in fs::read_dir(backups_root).map_err(|_| WorkspaceBackupError::Io)? {
        let entry = entry.map_err(|_| WorkspaceBackupError::Io)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".restore-staging-") || name.starts_with(".restore-rollback-") {
            return Err(WorkspaceBackupError::RestoreTargetUnsafe);
        }
    }
    Ok(())
}

fn validate_restore_targets(
    workspace_root: &Path,
    staging_root: &Path,
) -> Result<(), WorkspaceBackupError> {
    for root in MANAGED_ROOTS {
        let components = root.iter().map(|part| (*part).to_string()).collect::<Vec<_>>();
        let staged = join_components(staging_root, &components)?;
        let staged_metadata =
            fs::symlink_metadata(&staged).map_err(|_| WorkspaceBackupError::RestoreTargetUnsafe)?;
        if staged_metadata.file_type().is_symlink() || !staged_metadata.is_dir() {
            return Err(WorkspaceBackupError::RestoreTargetUnsafe);
        }

        let current = join_components(workspace_root, &components)?;
        let optional = root.len() == 2 && root[0] == "state" && root[1] == "autosave";
        validate_current_target(workspace_root, &current, optional)?;
    }

    Ok(())
}

fn validate_current_target(
    workspace_root: &Path,
    target: &Path,
    optional: bool,
) -> Result<(), WorkspaceBackupError> {
    let parent = target.parent().ok_or(WorkspaceBackupError::RestoreTargetUnsafe)?;
    if !parent.starts_with(workspace_root) {
        return Err(WorkspaceBackupError::RestoreTargetUnsafe);
    }

    if parent != workspace_root {
        let metadata =
            fs::symlink_metadata(parent).map_err(|_| WorkspaceBackupError::RestoreTargetUnsafe)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorkspaceBackupError::RestoreTargetUnsafe);
        }
    }

    match fs::symlink_metadata(target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(WorkspaceBackupError::RestoreTargetUnsafe);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && optional => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(WorkspaceBackupError::RestoreTargetUnsafe)
        }
        Err(_) => return Err(WorkspaceBackupError::RestoreTargetUnsafe),
    }

    Ok(())
}

#[derive(Debug)]
struct Replacement {
    relative: Vec<String>,
    had_original: bool,
    installed: bool,
}

fn replace_managed_roots(
    workspace_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    backups_root: &Path,
) -> Result<(), WorkspaceBackupError> {
    let mut replacements = Vec::<Replacement>::new();

    for root in MANAGED_ROOTS {
        let relative = root.iter().map(|part| (*part).to_string()).collect::<Vec<_>>();
        let current = join_components(workspace_root, &relative)?;
        let staged = join_components(staging_root, &relative)?;
        let rollback = join_components(rollback_root, &relative)?;

        if let Some(parent) = rollback.parent() {
            fs::create_dir_all(parent).map_err(|_| WorkspaceBackupError::RestoreFailed)?;
        }

        let had_original = path_exists(&current)?;
        let mut replacement = Replacement {
            relative: relative.clone(),
            had_original,
            installed: false,
        };

        if had_original {
            if fs::rename(&current, &rollback).is_err() {
                rollback_replacements(workspace_root, staging_root, rollback_root, &mut replacements);
                return Err(WorkspaceBackupError::RestoreFailed);
            }
        }

        if fs::rename(&staged, &current).is_err() {
            if had_original {
                let _ = fs::rename(&rollback, &current);
            }
            rollback_replacements(workspace_root, staging_root, rollback_root, &mut replacements);
            return Err(WorkspaceBackupError::RestoreFailed);
        }

        replacement.installed = true;
        replacements.push(replacement);

        if let Some(parent) = current.parent() {
            let _ = sync_directory(parent);
        }
        let _ = sync_directory(backups_root);
    }

    Ok(())
}

fn rollback_replacements(
    workspace_root: &Path,
    staging_root: &Path,
    rollback_root: &Path,
    replacements: &mut [Replacement],
) {
    for replacement in replacements.iter_mut().rev() {
        let Ok(current) = join_components(workspace_root, &replacement.relative) else {
            continue;
        };
        let Ok(staged) = join_components(staging_root, &replacement.relative) else {
            continue;
        };
        let Ok(rollback) = join_components(rollback_root, &replacement.relative) else {
            continue;
        };

        if replacement.installed {
            if let Some(parent) = staged.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(&current, &staged);
            replacement.installed = false;
        }

        if replacement.had_original {
            if let Some(parent) = current.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(&rollback, &current);
        }
    }
}

fn map_io(_error: io::Error) -> WorkspaceBackupError {
    WorkspaceBackupError::Io
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), WorkspaceBackupError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| WorkspaceBackupError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), WorkspaceBackupError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let result = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .and_then(|directory| directory.sync_all());

    let _ = result;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> Result<(), WorkspaceBackupError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-workspace-backup-{label}-{}-{unique}",
            std::process::id()
        ));

        for relative in [
            "data/documents",
            "data/exports",
            "data/received",
            "repositories",
            "backups",
            "state",
            "identity",
            "config",
        ] {
            fs::create_dir_all(root.join(relative)).expect("layout");
        }

        root
    }

    #[test]
    fn create_verify_and_restore_round_trip_managed_workspace_data() {
        let root = temporary_workspace("roundtrip");
        fs::create_dir_all(root.join("state/autosave")).expect("autosave");
        fs::create_dir_all(root.join("repositories/demo/.git")).expect("git");
        fs::write(root.join("data/documents/note.txt"), "before\n").expect("document");
        fs::write(root.join("repositories/demo/source.bin"), [0, 1, 2, 3]).expect("repo file");
        fs::write(root.join("repositories/demo/.git/HEAD"), "ref: refs/heads/main\n").expect("head");
        fs::write(root.join("identity/device.txt"), "identity").expect("identity");
        fs::write(root.join("config/settings.toml"), "mode = \"local\"\n").expect("config");
        fs::write(root.join("state/autosave/draft.txt"), "draft").expect("draft");
        fs::write(root.join("state/session.v1"), "current-session").expect("session");

        let created = create_backup(&root).expect("create");
        let summary = verify_backup(&root, &created.relative_path).expect("verify");
        assert!(summary.files >= 6);
        assert!(summary.directories >= 5);

        fs::write(root.join("data/documents/note.txt"), "after\n").expect("mutate");
        fs::remove_file(root.join("identity/device.txt")).expect("remove identity");
        fs::write(root.join("config/new.txt"), "new").expect("new config");
        fs::write(root.join("state/autosave/draft.txt"), "new draft").expect("new draft");
        fs::write(root.join("state/session.v1"), "new-session").expect("new session");

        let restored = restore_backup(&root, &created.relative_path).expect("restore");
        assert_eq!(restored, summary);
        assert_eq!(
            fs::read_to_string(root.join("data/documents/note.txt")).expect("document"),
            "before\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("identity/device.txt")).expect("identity"),
            "identity"
        );
        assert!(!root.join("config/new.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("state/autosave/draft.txt")).expect("draft"),
            "draft"
        );
        assert_eq!(
            fs::read_to_string(root.join("state/session.v1")).expect("session"),
            "new-session"
        );
        assert!(root.join(&created.relative_path).is_file());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn backup_excludes_backups_and_session_state() {
        let root = temporary_workspace("scope");
        fs::write(root.join("data/documents/note.txt"), "keep").expect("document");
        fs::write(root.join("backups/not-part-of-snapshot.txt"), "exclude").expect("other backup");
        fs::write(root.join("state/session.v1"), "exclude-session").expect("session");

        let created = create_backup(&root).expect("create");
        fs::write(root.join("backups/not-part-of-snapshot.txt"), "still-here").expect("mutate backup");
        fs::write(root.join("state/session.v1"), "still-current").expect("mutate session");

        restore_backup(&root, &created.relative_path).expect("restore");

        assert_eq!(
            fs::read_to_string(root.join("backups/not-part-of-snapshot.txt")).expect("backup file"),
            "still-here"
        );
        assert_eq!(
            fs::read_to_string(root.join("state/session.v1")).expect("session"),
            "still-current"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn tampered_backup_is_rejected_before_workspace_mutation() {
        let root = temporary_workspace("tamper");
        fs::write(root.join("data/documents/note.txt"), "backup-value").expect("document");
        let created = create_backup(&root).expect("create");

        let backup_path = root.join(&created.relative_path);
        let mut bytes = fs::read(&backup_path).expect("read backup");
        let index = bytes.len() / 2;
        bytes[index] ^= 0x5a;
        fs::write(&backup_path, bytes).expect("tamper");

        fs::write(root.join("data/documents/note.txt"), "current-value").expect("current");
        assert!(
            verify_backup(&root, &created.relative_path).is_err(),
            "tampered archive must never verify successfully"
        );
        assert!(
            restore_backup(&root, &created.relative_path).is_err(),
            "tampered archive must never reach a successful restore"
        );
        assert_eq!(
            fs::read_to_string(root.join("data/documents/note.txt")).expect("unchanged"),
            "current-value"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn list_backups_returns_only_direct_final_backup_files() {
        let root = temporary_workspace("list");
        fs::write(root.join("data/documents/note.txt"), "data").expect("document");
        let created = create_backup(&root).expect("create");
        fs::write(root.join("backups/other.txt"), "ignore").expect("other");
        fs::write(root.join("backups/.partial.dgmwbk.pending"), "ignore").expect("pending");

        let backups = list_backups(&root).expect("list");
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].relative_path, created.relative_path);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn restore_requires_direct_backup_path() {
        let root = temporary_workspace("path");
        assert_eq!(
            verify_backup(&root, Path::new("../outside.dgmwbk")).unwrap_err(),
            WorkspaceBackupError::InvalidBackupPath
        );
        assert_eq!(
            verify_backup(&root, Path::new("backups/nested/file.dgmwbk")).unwrap_err(),
            WorkspaceBackupError::InvalidBackupPath
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn empty_autosave_is_part_of_snapshot_and_clears_newer_drafts_on_restore() {
        let root = temporary_workspace("empty-autosave");
        fs::write(root.join("data/documents/note.txt"), "data").expect("document");
        let created = create_backup(&root).expect("create");

        fs::create_dir_all(root.join("state/autosave")).expect("autosave");
        fs::write(root.join("state/autosave/new.draft"), "newer").expect("draft");

        restore_backup(&root, &created.relative_path).expect("restore");
        assert!(root.join("state/autosave").is_dir());
        assert_eq!(
            fs::read_dir(root.join("state/autosave"))
                .expect("read autosave")
                .count(),
            0
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn backup_rejects_symlink_entries_in_managed_roots() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink-source");
        let outside = root.with_extension("outside");
        fs::write(&outside, "outside").expect("outside");
        symlink(&outside, root.join("data/documents/link.txt")).expect("link");

        assert_eq!(
            create_backup(&root).unwrap_err(),
            WorkspaceBackupError::UnsupportedEntry
        );

        fs::remove_file(root.join("data/documents/link.txt")).expect("remove link");
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_file(outside).expect("cleanup outside");
    }

    #[cfg(unix)]
    #[test]
    fn restore_refuses_unsafe_current_target_before_mutation() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("unsafe-target");
        fs::write(root.join("data/documents/note.txt"), "backup").expect("document");
        let created = create_backup(&root).expect("create");

        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).expect("outside");
        fs::remove_dir_all(root.join("identity")).expect("remove identity");
        symlink(&outside, root.join("identity")).expect("identity link");
        fs::write(root.join("data/documents/note.txt"), "current").expect("current");

        assert_eq!(
            restore_backup(&root, &created.relative_path).unwrap_err(),
            WorkspaceBackupError::RestoreTargetUnsafe
        );
        assert_eq!(
            fs::read_to_string(root.join("data/documents/note.txt")).expect("unchanged"),
            "current"
        );

        fs::remove_file(root.join("identity")).expect("remove identity link");
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
