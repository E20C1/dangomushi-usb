use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_TEXT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const RECOVERY_MAGIC: &str = "DANGOMUSHI_RECOVERY_V1";
const RECOVERY_DIR: &[&str] = &["state", "autosave"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDraft {
    pub text: String,
    pub file_path: PathBuf,
}

pub fn read_text(root: &Path, relative: &Path) -> io::Result<String> {
    let file_path = resolve_existing_file(root, relative)?;
    let metadata = fs::metadata(&file_path)?;

    if metadata.len() > MAX_TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "text file exceeds editor limit of {} bytes",
                MAX_TEXT_FILE_BYTES
            ),
        ));
    }

    let bytes = fs::read(file_path)?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "file is not valid UTF-8 text",
        )
    })
}

pub fn save_text(root: &Path, relative: &Path, text: &str) -> io::Result<()> {
    if text.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "text exceeds editor limit of {} bytes",
                MAX_TEXT_FILE_BYTES
            ),
        ));
    }

    // Persist a recovery copy before touching the original file. If the
    // following write or device flush fails, the user's current buffer remains
    // recoverable from workspace/state/autosave.
    write_recovery(root, relative, text)?;

    let file_path = resolve_existing_file(root, relative)?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(file_path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;

    // A stale recovery is harmless, so failure to remove it must not turn a
    // successful explicit save into a reported save failure.
    let _ = clear_recovery(root, relative);
    Ok(())
}

pub fn write_recovery(root: &Path, relative: &Path, text: &str) -> io::Result<PathBuf> {
    validate_file_relative(relative)?;

    if text.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "recovery text exceeds editor size limit",
        ));
    }

    // Validate that the edited source itself is still a regular in-workspace
    // file before creating a recovery draft for it.
    resolve_existing_file(root, relative)?;

    let recovery_dir = ensure_recovery_dir(root)?;
    let prefix = recovery_prefix(relative);
    let path_hex = hex_encode(normalized_relative(relative).as_bytes());
    let payload = format!("{RECOVERY_MAGIC}\nPATH_HEX:{path_hex}\n\n{text}");

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let mut created = None;
    for counter in 0..1000_u16 {
        let candidate = recovery_dir.join(format!(
            "{prefix}{millis:020}-{counter:03}.draft"
        ));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(payload.as_bytes())?;
                file.sync_all()?;
                created = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    let created = created.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique recovery filename",
        )
    })?;

    // New draft is already durable. Older drafts can now be removed. If cleanup
    // fails, leaving an older draft is safer than failing the autosave.
    cleanup_older_recoveries(&recovery_dir, relative, &created);
    Ok(created)
}

pub fn latest_recovery(root: &Path, relative: &Path) -> io::Result<Option<RecoveryDraft>> {
    validate_file_relative(relative)?;
    resolve_existing_file(root, relative)?;

    let recovery_dir = ensure_recovery_dir(root)?;
    let prefix = recovery_prefix(relative);
    let expected_path_hex = hex_encode(normalized_relative(relative).as_bytes());

    let mut candidates = Vec::new();
    for entry in fs::read_dir(&recovery_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".draft") {
            candidates.push((name, entry.path()));
        }
    }

    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    for (_, path) in candidates {
        match read_recovery_file(&path, &expected_path_hex) {
            Ok(Some(text)) => {
                return Ok(Some(RecoveryDraft {
                    text,
                    file_path: path,
                }))
            }
            Ok(None) => continue,
            Err(_) => continue,
        }
    }

    Ok(None)
}

pub fn clear_recovery(root: &Path, relative: &Path) -> io::Result<()> {
    validate_file_relative(relative)?;
    let recovery_dir = ensure_recovery_dir(root)?;
    let prefix = recovery_prefix(relative);
    let expected_path_hex = hex_encode(normalized_relative(relative).as_bytes());

    for entry in fs::read_dir(recovery_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !name.ends_with(".draft") {
            continue;
        }

        if matches!(
            read_recovery_file(&entry.path(), &expected_path_hex),
            Ok(Some(_))
        ) {
            let _ = fs::remove_file(entry.path());
        }
    }

    Ok(())
}

fn cleanup_older_recoveries(recovery_dir: &Path, relative: &Path, keep: &Path) {
    let prefix = recovery_prefix(relative);
    let expected_path_hex = hex_encode(normalized_relative(relative).as_bytes());

    let Ok(entries) = fs::read_dir(recovery_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !name.ends_with(".draft") {
            continue;
        }

        if matches!(read_recovery_file(&path, &expected_path_hex), Ok(Some(_))) {
            let _ = fs::remove_file(path);
        }
    }
}

fn read_recovery_file(path: &Path, expected_path_hex: &str) -> io::Result<Option<String>> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let Some((header, body)) = content.split_once("\n\n") else {
        return Ok(None);
    };

    let mut lines = header.lines();
    if lines.next() != Some(RECOVERY_MAGIC) {
        return Ok(None);
    }

    let Some(path_line) = lines.next() else {
        return Ok(None);
    };
    let Some(path_hex) = path_line.strip_prefix("PATH_HEX:") else {
        return Ok(None);
    };

    if path_hex != expected_path_hex {
        return Ok(None);
    }

    Ok(Some(body.to_string()))
}

fn resolve_existing_file(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    validate_file_relative(relative)?;
    let canonical_root = fs::canonicalize(root)?;
    let candidate = walk_without_symlinks(&canonical_root, relative)?;
    let canonical_candidate = fs::canonicalize(&candidate)?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file resolves outside the workspace root",
        ));
    }

    if !canonical_candidate.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }

    Ok(canonical_candidate)
}

fn ensure_recovery_dir(root: &Path) -> io::Result<PathBuf> {
    let canonical_root = fs::canonicalize(root)?;
    let mut cursor = canonical_root.clone();

    for component in RECOVERY_DIR {
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "recovery directory path contains a symlink",
                    ));
                }
                if !metadata.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "recovery path component is not a directory",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&cursor)?;
            }
            Err(error) => return Err(error),
        }
    }

    let canonical_recovery = fs::canonicalize(&cursor)?;
    if !canonical_recovery.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recovery directory resolves outside workspace",
        ));
    }

    Ok(canonical_recovery)
}

fn walk_without_symlinks(canonical_root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let mut cursor = canonical_root.to_path_buf();

    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace file path contains an invalid component",
            ));
        };

        cursor.push(segment);
        let metadata = fs::symlink_metadata(&cursor)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "workspace file path contains a symlink",
            ));
        }
    }

    Ok(cursor)
}

fn validate_file_relative(relative: &Path) -> io::Result<()> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "editor requires a non-empty relative workspace path",
        ));
    }

    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace paths may contain only normal relative components",
        ));
    }

    Ok(())
}

fn normalized_relative(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

fn recovery_prefix(relative: &Path) -> String {
    let hash = fnv1a64(normalized_relative(relative).as_bytes());
    format!("rec-{hash:016x}-")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn hex_encode(bytes: &[u8]) -> String {
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-editor-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("state")).expect("state");
        root
    }

    #[test]
    fn reads_and_explicitly_saves_utf8_text() {
        let root = temporary_workspace("save");
        fs::create_dir_all(root.join("docs")).expect("docs");
        fs::write(root.join("docs/note.txt"), "before").expect("seed");

        assert_eq!(
            read_text(&root, Path::new("docs/note.txt")).expect("read"),
            "before"
        );

        save_text(&root, Path::new("docs/note.txt"), "after\n日本語").expect("save");

        assert_eq!(
            read_text(&root, Path::new("docs/note.txt")).expect("read saved"),
            "after\n日本語"
        );
        assert!(
            latest_recovery(&root, Path::new("docs/note.txt"))
                .expect("recovery lookup")
                .is_none()
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn recovery_round_trip_keeps_original_untouched() {
        let root = temporary_workspace("recovery");
        fs::write(root.join("note.txt"), "disk").expect("seed");

        write_recovery(&root, Path::new("note.txt"), "draft").expect("write recovery");

        assert_eq!(
            read_text(&root, Path::new("note.txt")).expect("disk read"),
            "disk"
        );

        let recovery = latest_recovery(&root, Path::new("note.txt"))
            .expect("lookup")
            .expect("draft exists");
        assert_eq!(recovery.text, "draft");

        clear_recovery(&root, Path::new("note.txt")).expect("clear");
        assert!(
            latest_recovery(&root, Path::new("note.txt"))
                .expect("lookup after clear")
                .is_none()
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_parent_traversal() {
        let root = temporary_workspace("traversal");
        let error = read_text(&root, Path::new("../outside.txt")).expect_err("reject");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_non_utf8_content() {
        let root = temporary_workspace("binary");
        fs::write(root.join("binary.dat"), [0xff, 0xfe, 0xfd]).expect("seed");
        let error = read_text(&root, Path::new("binary.dat")).expect_err("reject binary");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_files() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink");
        let outside = root.with_extension("outside");
        fs::write(&outside, "outside").expect("outside");
        symlink(&outside, root.join("link.txt")).expect("symlink");

        let error = read_text(&root, Path::new("link.txt")).expect_err("reject link");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_file(outside).expect("cleanup outside");
    }

    #[test]
    fn recovery_filename_is_stable_for_a_path() {
        assert_eq!(
            recovery_prefix(Path::new("docs/note.txt")),
            recovery_prefix(Path::new("docs/note.txt"))
        );
        assert_ne!(
            recovery_prefix(Path::new("docs/note.txt")),
            recovery_prefix(Path::new("docs/other.txt"))
        );
    }

    #[test]
    fn hex_encoding_is_unambiguous() {
        assert_eq!(hex_encode(b"a/b"), "612f62");
        assert_eq!(hex_encode(OsStr::new("").to_string_lossy().as_bytes()), "");
    }
}
