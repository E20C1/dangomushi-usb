use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Directory => "DIR",
            Self::File => "FILE",
            Self::Symlink => "LINK",
            Self::Other => "OTHER",
        }
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::Directory => 0,
            Self::File => 1,
            Self::Symlink => 2,
            Self::Other => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub relative_path: PathBuf,
    pub kind: EntryKind,
    pub size_bytes: u64,
}

pub fn list_directory(root: &Path, relative: &Path) -> io::Result<Vec<FileEntry>> {
    validate_relative(relative)?;
    let directory = resolve_directory(root, relative)?;
    let mut entries = Vec::new();

    for item in fs::read_dir(directory)? {
        let item = item?;
        let metadata = fs::symlink_metadata(item.path())?;
        let file_type = metadata.file_type();

        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };

        let name = item.file_name().to_string_lossy().into_owned();
        let relative_path = relative.join(item.file_name());

        entries.push(FileEntry {
            name,
            relative_path,
            kind,
            size_bytes: if kind == EntryKind::File {
                metadata.len()
            } else {
                0
            },
        });
    }

    entries.sort_by(|left, right| {
        left.kind
            .sort_rank()
            .cmp(&right.kind.sort_rank())
            .then_with(|| {
                left.name
                    .to_lowercase()
                    .cmp(&right.name.to_lowercase())
            })
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(entries)
}

pub fn parent(relative: &Path) -> PathBuf {
    relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf()
}

pub fn display_path(relative: &Path) -> String {
    if relative.as_os_str().is_empty() {
        "workspace:/".to_string()
    } else {
        format!("workspace:/{}", relative.to_string_lossy().replace('\\', "/"))
    }
}

fn resolve_directory(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let canonical_root = fs::canonicalize(root)?;
    let candidate = canonical_root.join(relative);
    let canonical_candidate = fs::canonicalize(candidate)?;

    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path resolves outside the workspace root",
        ));
    }

    if !canonical_candidate.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "path is not a directory",
        ));
    }

    Ok(canonical_candidate)
}

fn validate_relative(relative: &Path) -> io::Result<()> {
    if relative.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute paths are not allowed",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-browser-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn lists_directories_before_files_and_keeps_paths_relative() {
        let root = temporary_directory("listing");
        fs::create_dir_all(root.join("beta")).expect("create beta");
        fs::create_dir_all(root.join("alpha")).expect("create alpha");
        fs::write(root.join("note.txt"), b"1234").expect("write file");

        let entries = list_directory(&root, Path::new("")).expect("list root");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[0].relative_path, PathBuf::from("alpha"));
        assert_eq!(entries[1].name, "beta");
        assert_eq!(entries[2].name, "note.txt");
        assert_eq!(entries[2].kind, EntryKind::File);
        assert_eq!(entries[2].size_bytes, 4);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_parent_traversal() {
        let root = temporary_directory("traversal");
        fs::create_dir_all(&root).expect("create root");

        let error = list_directory(&root, Path::new("..")).expect_err("reject traversal");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_absolute_paths() {
        let root = temporary_directory("absolute");
        fs::create_dir_all(&root).expect("create root");

        let absolute = fs::canonicalize(&root).expect("canonical root");
        let error = list_directory(&root, &absolute).expect_err("reject absolute");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_directory_that_resolves_outside_root() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory("symlink-root");
        let outside = temporary_directory("symlink-outside");
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&outside).expect("create outside");
        symlink(&outside, root.join("outside-link")).expect("create symlink");

        let root_entries = list_directory(&root, Path::new("")).expect("list root");
        assert_eq!(root_entries[0].kind, EntryKind::Symlink);

        let error =
            list_directory(&root, Path::new("outside-link")).expect_err("reject outside symlink");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
