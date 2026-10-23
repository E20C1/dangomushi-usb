use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

const READ_BUFFER_BYTES: usize = 64 * 1024;
pub const TEXT_DETAIL_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextComparison {
    pub left_lines: usize,
    pub right_lines: usize,
    pub first_differing_line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileComparison {
    pub equal: bool,
    pub left_bytes: u64,
    pub right_bytes: u64,
    pub first_difference_byte: Option<u64>,
    pub text: Option<TextComparison>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCompareError {
    InvalidWorkspacePath,
    UnsafePath,
    TargetNotRegularFile,
    Io,
}

impl fmt::Display for FileCompareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidWorkspacePath => {
                "comparison target must be a normal relative workspace path"
            }
            Self::UnsafePath => "comparison target resolves through an unsafe filesystem path",
            Self::TargetNotRegularFile => "comparison target is not a regular file",
            Self::Io => "file comparison failed",
        };
        formatter.write_str(message)
    }
}

impl Error for FileCompareError {}

pub fn compare_workspace_files(
    workspace_root: &Path,
    left_relative: &Path,
    right_relative: &Path,
) -> Result<FileComparison, FileCompareError> {
    let left_path = resolve_existing_file(workspace_root, left_relative)?;
    let right_path = resolve_existing_file(workspace_root, right_relative)?;

    let left_metadata = fs::metadata(&left_path).map_err(|_| FileCompareError::Io)?;
    let right_metadata = fs::metadata(&right_path).map_err(|_| FileCompareError::Io)?;

    let left_bytes = left_metadata.len();
    let right_bytes = right_metadata.len();

    let left_file = File::open(&left_path).map_err(|_| FileCompareError::Io)?;
    let right_file = File::open(&right_path).map_err(|_| FileCompareError::Io)?;

    let first_difference_byte = first_difference(
        BufReader::with_capacity(READ_BUFFER_BYTES, left_file),
        BufReader::with_capacity(READ_BUFFER_BYTES, right_file),
    )?;

    let text = if left_bytes <= TEXT_DETAIL_MAX_BYTES && right_bytes <= TEXT_DETAIL_MAX_BYTES {
        text_comparison(&left_path, &right_path).ok().flatten()
    } else {
        None
    };

    Ok(FileComparison {
        equal: first_difference_byte.is_none(),
        left_bytes,
        right_bytes,
        first_difference_byte,
        text,
    })
}

fn first_difference(
    mut left: BufReader<File>,
    mut right: BufReader<File>,
) -> Result<Option<u64>, FileCompareError> {
    let mut offset = 0_u64;

    loop {
        let left_buf = left.fill_buf().map_err(|_| FileCompareError::Io)?;
        let right_buf = right.fill_buf().map_err(|_| FileCompareError::Io)?;

        if left_buf.is_empty() && right_buf.is_empty() {
            return Ok(None);
        }

        if left_buf.is_empty() || right_buf.is_empty() {
            return Ok(Some(offset));
        }

        let common = left_buf.len().min(right_buf.len());

        if let Some(position) = left_buf[..common]
            .iter()
            .zip(&right_buf[..common])
            .position(|(left, right)| left != right)
        {
            return Ok(Some(
                offset
                    .checked_add(position as u64)
                    .ok_or(FileCompareError::Io)?,
            ));
        }

        left.consume(common);
        right.consume(common);
        offset = offset
            .checked_add(common as u64)
            .ok_or(FileCompareError::Io)?;
    }
}

fn text_comparison(
    left_path: &Path,
    right_path: &Path,
) -> Result<Option<TextComparison>, FileCompareError> {
    let left = read_small_utf8(left_path)?;
    let right = read_small_utf8(right_path)?;

    let (Some(left), Some(right)) = (left, right) else {
        return Ok(None);
    };

    let left_lines: Vec<&str> = split_lines_preserving_trailing(&left);
    let right_lines: Vec<&str> = split_lines_preserving_trailing(&right);

    let first_differing_line = first_line_difference(&left_lines, &right_lines);

    Ok(Some(TextComparison {
        left_lines: left_lines.len(),
        right_lines: right_lines.len(),
        first_differing_line,
    }))
}

fn read_small_utf8(path: &Path) -> Result<Option<String>, FileCompareError> {
    let file = File::open(path).map_err(|_| FileCompareError::Io)?;
    let mut limited = file.take(TEXT_DETAIL_MAX_BYTES + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| FileCompareError::Io)?;

    if bytes.len() as u64 > TEXT_DETAIL_MAX_BYTES {
        return Ok(None);
    }

    Ok(String::from_utf8(bytes).ok())
}

fn split_lines_preserving_trailing(input: &str) -> Vec<&str> {
    if input.is_empty() {
        Vec::new()
    } else {
        input.split('\n').collect()
    }
}

fn first_line_difference(left: &[&str], right: &[&str]) -> Option<usize> {
    let common = left.len().min(right.len());

    for index in 0..common {
        if left[index] != right[index] {
            return Some(index + 1);
        }
    }

    if left.len() != right.len() {
        Some(common + 1)
    } else {
        None
    }
}

fn resolve_existing_file(
    workspace_root: &Path,
    relative: &Path,
) -> Result<PathBuf, FileCompareError> {
    validate_relative_path(relative)?;

    let canonical_root =
        fs::canonicalize(workspace_root).map_err(|_| FileCompareError::Io)?;
    let root_metadata = fs::metadata(&canonical_root).map_err(|_| FileCompareError::Io)?;
    if !root_metadata.is_dir() {
        return Err(FileCompareError::InvalidWorkspacePath);
    }

    let mut candidate = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(FileCompareError::InvalidWorkspacePath);
        };

        candidate.push(segment);
        let metadata = fs::symlink_metadata(&candidate).map_err(|_| FileCompareError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(FileCompareError::UnsafePath);
        }
    }

    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|_| FileCompareError::Io)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(FileCompareError::UnsafePath);
    }

    let metadata = fs::metadata(&canonical_candidate).map_err(|_| FileCompareError::Io)?;
    if !metadata.is_file() {
        return Err(FileCompareError::TargetNotRegularFile);
    }

    Ok(canonical_candidate)
}

fn validate_relative_path(relative: &Path) -> Result<(), FileCompareError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(FileCompareError::InvalidWorkspacePath);
    }

    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(FileCompareError::InvalidWorkspacePath);
    }

    Ok(())
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
            "dangomushi-compare-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn equal_binary_files_compare_equal() {
        let root = temporary_workspace("equal");
        fs::write(root.join("a.bin"), [0_u8, 1, 2, 3, 255]).expect("left");
        fs::write(root.join("b.bin"), [0_u8, 1, 2, 3, 255]).expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.bin"),
            Path::new("b.bin"),
        )
        .expect("compare");

        assert!(result.equal);
        assert_eq!(result.first_difference_byte, None);
        assert_eq!(result.left_bytes, 5);
        assert_eq!(result.right_bytes, 5);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn reports_first_differing_byte() {
        let root = temporary_workspace("difference");
        fs::write(root.join("a.bin"), b"abcXdef").expect("left");
        fs::write(root.join("b.bin"), b"abcYdef").expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.bin"),
            Path::new("b.bin"),
        )
        .expect("compare");

        assert!(!result.equal);
        assert_eq!(result.first_difference_byte, Some(3));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn prefix_length_difference_reports_end_of_shorter_file() {
        let root = temporary_workspace("length");
        fs::write(root.join("a.bin"), b"abc").expect("left");
        fs::write(root.join("b.bin"), b"abcx").expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.bin"),
            Path::new("b.bin"),
        )
        .expect("compare");

        assert_eq!(result.first_difference_byte, Some(3));
        assert_eq!(result.left_bytes, 3);
        assert_eq!(result.right_bytes, 4);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn utf8_text_reports_line_counts_and_first_differing_line() {
        let root = temporary_workspace("text");
        fs::write(root.join("a.txt"), "one\ntwo\nthree\n").expect("left");
        fs::write(root.join("b.txt"), "one\nTWO\nthree\n").expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.txt"),
            Path::new("b.txt"),
        )
        .expect("compare");

        assert_eq!(
            result.text,
            Some(TextComparison {
                left_lines: 4,
                right_lines: 4,
                first_differing_line: Some(2),
            })
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn binary_files_do_not_claim_text_detail() {
        let root = temporary_workspace("binary");
        fs::write(root.join("a.bin"), [0xff, 0xfe]).expect("left");
        fs::write(root.join("b.bin"), [0xff, 0xfd]).expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.bin"),
            Path::new("b.bin"),
        )
        .expect("compare");

        assert_eq!(result.text, None);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn large_files_compare_streaming() {
        let root = temporary_workspace("large");
        let mut left = vec![b'a'; READ_BUFFER_BYTES * 3 + 17];
        let mut right = left.clone();
        left[READ_BUFFER_BYTES * 2 + 7] = b'x';
        right[READ_BUFFER_BYTES * 2 + 7] = b'y';

        fs::write(root.join("a.bin"), &left).expect("left");
        fs::write(root.join("b.bin"), &right).expect("right");

        let result = compare_workspace_files(
            &root,
            Path::new("a.bin"),
            Path::new("b.bin"),
        )
        .expect("compare");

        assert_eq!(
            result.first_difference_byte,
            Some((READ_BUFFER_BYTES * 2 + 7) as u64)
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_parent_traversal_and_directories() {
        let root = temporary_workspace("path");
        fs::create_dir(root.join("folder")).expect("folder");
        fs::write(root.join("a.bin"), b"a").expect("left");

        assert_eq!(
            compare_workspace_files(
                &root,
                Path::new("../outside"),
                Path::new("a.bin"),
            )
            .unwrap_err(),
            FileCompareError::InvalidWorkspacePath
        );

        assert_eq!(
            compare_workspace_files(
                &root,
                Path::new("folder"),
                Path::new("a.bin"),
            )
            .unwrap_err(),
            FileCompareError::TargetNotRegularFile
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_targets() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("symlink");
        let outside = root.with_extension("outside");
        fs::write(&outside, b"outside").expect("outside");
        fs::write(root.join("a.bin"), b"a").expect("left");
        symlink(&outside, root.join("link.bin")).expect("link");

        assert_eq!(
            compare_workspace_files(
                &root,
                Path::new("a.bin"),
                Path::new("link.bin"),
            )
            .unwrap_err(),
            FileCompareError::UnsafePath
        );

        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_file(outside).expect("cleanup outside");
    }
}
