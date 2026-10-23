use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_SEARCH_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_FILES_CONSIDERED: usize = 10_000;
pub const MAX_SEARCH_RESULTS: usize = 500;
pub const MAX_PREVIEW_CHARS: usize = 240;

const DOCUMENTS_RELATIVE: &[&str] = &["data", "documents"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSearchMatch {
    pub relative_path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSearchReport {
    pub matches: Vec<DocumentSearchMatch>,
    pub files_considered: usize,
    pub files_searched: usize,
    pub skipped_large: usize,
    pub skipped_non_utf8: usize,
    pub skipped_symlinks: usize,
    pub result_limit_reached: bool,
    pub file_limit_reached: bool,
}

impl DocumentSearchReport {
    fn new() -> Self {
        Self {
            matches: Vec::new(),
            files_considered: 0,
            files_searched: 0,
            skipped_large: 0,
            skipped_non_utf8: 0,
            skipped_symlinks: 0,
            result_limit_reached: false,
            file_limit_reached: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentSearchError {
    InvalidQuery,
    DocumentsRootUnavailable,
    UnsafeDocumentsRoot,
    Io,
}

impl fmt::Display for DocumentSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidQuery => "document search query is invalid",
            Self::DocumentsRootUnavailable => "documents search root is unavailable",
            Self::UnsafeDocumentsRoot => "documents search root is unsafe",
            Self::Io => "document search failed",
        };
        formatter.write_str(message)
    }
}

impl Error for DocumentSearchError {}

pub fn search_documents(
    workspace_root: &Path,
    query: &str,
) -> Result<DocumentSearchReport, DocumentSearchError> {
    validate_query(query)?;
    let (canonical_workspace, documents_root) = checked_documents_root(workspace_root)?;

    let mut report = DocumentSearchReport::new();
    let mut directories = VecDeque::new();
    directories.push_back(documents_root.clone());

    while let Some(directory) = directories.pop_front() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|_| DocumentSearchError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| DocumentSearchError::Io)?;

        entries.sort_by(|left, right| {
            left.file_name()
                .to_string_lossy()
                .cmp(&right.file_name().to_string_lossy())
        });

        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| DocumentSearchError::Io)?;

            if metadata.file_type().is_symlink() {
                report.skipped_symlinks = report.skipped_symlinks.saturating_add(1);
                continue;
            }

            if metadata.is_dir() {
                let canonical =
                    fs::canonicalize(&path).map_err(|_| DocumentSearchError::Io)?;
                if !canonical.starts_with(&documents_root) {
                    return Err(DocumentSearchError::UnsafeDocumentsRoot);
                }
                directories.push_back(canonical);
                continue;
            }

            if !metadata.is_file() {
                continue;
            }

            if report.files_considered >= MAX_FILES_CONSIDERED {
                report.file_limit_reached = true;
                return Ok(report);
            }
            report.files_considered += 1;

            let canonical = fs::canonicalize(&path).map_err(|_| DocumentSearchError::Io)?;
            if !canonical.starts_with(&documents_root)
                || !canonical.starts_with(&canonical_workspace)
            {
                return Err(DocumentSearchError::UnsafeDocumentsRoot);
            }

            if metadata.len() > MAX_DOCUMENT_BYTES {
                report.skipped_large += 1;
                continue;
            }

            let Some(text) = read_bounded_utf8(&canonical)? else {
                let actual_len = fs::metadata(&canonical)
                    .map_err(|_| DocumentSearchError::Io)?
                    .len();
                if actual_len > MAX_DOCUMENT_BYTES {
                    report.skipped_large += 1;
                } else {
                    report.skipped_non_utf8 += 1;
                }
                continue;
            };

            report.files_searched += 1;
            let relative = canonical
                .strip_prefix(&canonical_workspace)
                .map_err(|_| DocumentSearchError::UnsafeDocumentsRoot)?
                .to_path_buf();

            for (line_index, line) in text.lines().enumerate() {
                let Some(byte_column) = line.find(query) else {
                    continue;
                };

                let column = line[..byte_column].chars().count() + 1;
                report.matches.push(DocumentSearchMatch {
                    relative_path: relative.clone(),
                    line: line_index + 1,
                    column,
                    preview: make_preview(line, byte_column),
                });

                if report.matches.len() >= MAX_SEARCH_RESULTS {
                    report.result_limit_reached = true;
                    return Ok(report);
                }
            }
        }
    }

    Ok(report)
}

fn validate_query(query: &str) -> Result<(), DocumentSearchError> {
    if query.trim().is_empty()
        || query.len() > MAX_SEARCH_QUERY_BYTES
        || query.contains('\n')
        || query.contains('\r')
    {
        return Err(DocumentSearchError::InvalidQuery);
    }
    Ok(())
}

fn checked_documents_root(
    workspace_root: &Path,
) -> Result<(PathBuf, PathBuf), DocumentSearchError> {
    let canonical_workspace =
        fs::canonicalize(workspace_root).map_err(|_| DocumentSearchError::DocumentsRootUnavailable)?;

    if !canonical_workspace.is_dir() {
        return Err(DocumentSearchError::DocumentsRootUnavailable);
    }

    let mut cursor = canonical_workspace.clone();

    for component in DOCUMENTS_RELATIVE {
        cursor.push(component);
        let metadata = fs::symlink_metadata(&cursor)
            .map_err(|_| DocumentSearchError::DocumentsRootUnavailable)?;

        if metadata.file_type().is_symlink() {
            return Err(DocumentSearchError::UnsafeDocumentsRoot);
        }
        if !metadata.is_dir() {
            return Err(DocumentSearchError::DocumentsRootUnavailable);
        }
    }

    let canonical_documents =
        fs::canonicalize(&cursor).map_err(|_| DocumentSearchError::DocumentsRootUnavailable)?;

    if !canonical_documents.starts_with(&canonical_workspace) {
        return Err(DocumentSearchError::UnsafeDocumentsRoot);
    }

    Ok((canonical_workspace, canonical_documents))
}

fn read_bounded_utf8(path: &Path) -> Result<Option<String>, DocumentSearchError> {
    let file = File::open(path).map_err(|_| DocumentSearchError::Io)?;
    let mut limited = file.take(MAX_DOCUMENT_BYTES + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| DocumentSearchError::Io)?;

    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Ok(None);
    }

    Ok(String::from_utf8(bytes).ok())
}

fn make_preview(line: &str, match_byte: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let match_char = line[..match_byte].chars().count();
    let before = 60usize;
    let start = match_char.saturating_sub(before);
    let end = (start + MAX_PREVIEW_CHARS).min(chars.len());

    let mut preview = String::new();
    if start > 0 {
        preview.push('…');
    }
    for ch in &chars[start..end] {
        preview.push(*ch);
    }
    if end < chars.len() {
        preview.push('…');
    }
    preview
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
            "dangomushi-search-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("data/documents")).expect("documents");
        fs::create_dir_all(root.join("state/autosave")).expect("state");
        root
    }

    #[test]
    fn finds_literal_matches_recursively_with_line_and_column() {
        let root = temporary_workspace("recursive");
        fs::create_dir_all(root.join("data/documents/nested")).expect("nested");
        fs::write(
            root.join("data/documents/nested/note.txt"),
            "first\n日本語 target here\nlast\n",
        )
        .expect("document");

        let report = search_documents(&root, "target").expect("search");

        assert_eq!(report.matches.len(), 1);
        let found = &report.matches[0];
        assert_eq!(
            found.relative_path,
            PathBuf::from("data/documents/nested/note.txt")
        );
        assert_eq!(found.line, 2);
        assert_eq!(found.column, 5);
        assert!(found.preview.contains("target"));
        assert_eq!(report.files_considered, 1);
        assert_eq!(report.files_searched, 1);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn searches_documents_only_and_not_internal_workspace_state() {
        let root = temporary_workspace("scope");
        fs::write(root.join("data/documents/visible.txt"), "needle").expect("document");
        fs::write(root.join("state/autosave/hidden.txt"), "needle").expect("state");

        let report = search_documents(&root, "needle").expect("search");

        assert_eq!(report.matches.len(), 1);
        assert_eq!(
            report.matches[0].relative_path,
            PathBuf::from("data/documents/visible.txt")
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn skips_binary_and_oversized_files() {
        let root = temporary_workspace("skips");
        fs::write(root.join("data/documents/binary.bin"), [0xff, 0xfe, 0xfd])
            .expect("binary");
        fs::write(
            root.join("data/documents/large.txt"),
            vec![b'a'; MAX_DOCUMENT_BYTES as usize + 1],
        )
        .expect("large");

        let report = search_documents(&root, "a").expect("search");

        assert!(report.matches.is_empty());
        assert_eq!(report.skipped_non_utf8, 1);
        assert_eq!(report.skipped_large, 1);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_empty_multiline_and_oversized_queries() {
        let root = temporary_workspace("query");

        assert_eq!(
            search_documents(&root, "   ").unwrap_err(),
            DocumentSearchError::InvalidQuery
        );
        assert_eq!(
            search_documents(&root, "a\nb").unwrap_err(),
            DocumentSearchError::InvalidQuery
        );
        let long = "x".repeat(MAX_SEARCH_QUERY_BYTES + 1);
        assert_eq!(
            search_documents(&root, &long).unwrap_err(),
            DocumentSearchError::InvalidQuery
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn stops_at_result_limit() {
        let root = temporary_workspace("result-limit");
        let payload = (0..(MAX_SEARCH_RESULTS + 20))
            .map(|index| format!("needle {index}\n"))
            .collect::<String>();
        fs::write(root.join("data/documents/many.txt"), payload).expect("document");

        let report = search_documents(&root, "needle").expect("search");

        assert_eq!(report.matches.len(), MAX_SEARCH_RESULTS);
        assert!(report.result_limit_reached);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn results_are_deterministic_by_directory_entry_order() {
        let root = temporary_workspace("order");
        fs::write(root.join("data/documents/z.txt"), "needle").expect("z");
        fs::write(root.join("data/documents/a.txt"), "needle").expect("a");

        let report = search_documents(&root, "needle").expect("search");

        assert_eq!(
            report
                .matches
                .iter()
                .map(|item| item.relative_path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("data/documents/a.txt"),
                PathBuf::from("data/documents/z.txt")
            ]
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_inside_documents() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink-entry");
        let outside = root.with_extension("outside");
        fs::write(&outside, "needle").expect("outside");
        symlink(&outside, root.join("data/documents/link.txt")).expect("link");

        let report = search_documents(&root, "needle").expect("search");
        assert!(report.matches.is_empty());
        assert_eq!(report.skipped_symlinks, 1);

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_file(outside).expect("cleanup outside");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_documents_root() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink-root");
        fs::remove_dir(root.join("data/documents")).expect("remove documents");
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, root.join("data/documents")).expect("link");

        assert_eq!(
            search_documents(&root, "needle").unwrap_err(),
            DocumentSearchError::UnsafeDocumentsRoot
        );

        fs::remove_file(root.join("data/documents")).expect("remove link");
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
