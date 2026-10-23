use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const READ_BUFFER_BYTES: usize = 64 * 1024;
pub const SHA256_HEX_LENGTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn parse_hex(input: &str) -> Result<Self, HashVerifyError> {
        let input = input.trim();
        if input.len() != SHA256_HEX_LENGTH {
            return Err(HashVerifyError::InvalidExpectedDigest);
        }

        let mut bytes = [0_u8; 32];
        for (index, pair) in input.as_bytes().chunks_exact(2).enumerate() {
            let high = hex_value(pair[0]).ok_or(HashVerifyError::InvalidExpectedDigest)?;
            let low = hex_value(pair[1]).ok_or(HashVerifyError::InvalidExpectedDigest)?;
            bytes[index] = (high << 4) | low;
        }

        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(SHA256_HEX_LENGTH);
        for byte in self.0 {
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
        output
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileHash {
    pub sha256: Sha256Digest,
    pub bytes_hashed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashVerification {
    pub actual: FileHash,
    pub matches: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashVerifyError {
    InvalidWorkspacePath,
    UnsafePath,
    TargetNotRegularFile,
    InvalidExpectedDigest,
    Io,
}

impl fmt::Display for HashVerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidWorkspacePath => "hash target must be a normal relative workspace path",
            Self::UnsafePath => "hash target resolves through an unsafe filesystem path",
            Self::TargetNotRegularFile => "hash target is not a regular file",
            Self::InvalidExpectedDigest => {
                "expected SHA-256 digest must contain exactly 64 hexadecimal characters"
            }
            Self::Io => "hash operation failed",
        };
        formatter.write_str(message)
    }
}

impl Error for HashVerifyError {}

pub fn hash_workspace_file(
    workspace_root: &Path,
    relative: &Path,
) -> Result<FileHash, HashVerifyError> {
    let path = resolve_existing_file(workspace_root, relative)?;
    let mut file = File::open(path).map_err(|_| HashVerifyError::Io)?;
    let metadata = file.metadata().map_err(|_| HashVerifyError::Io)?;
    if !metadata.is_file() {
        return Err(HashVerifyError::TargetNotRegularFile);
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    let mut bytes_hashed = 0_u64;

    loop {
        let read = file.read(&mut buffer).map_err(|_| HashVerifyError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes_hashed = bytes_hashed
            .checked_add(read as u64)
            .ok_or(HashVerifyError::Io)?;
    }

    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);

    Ok(FileHash {
        sha256: Sha256Digest(bytes),
        bytes_hashed,
    })
}

pub fn verify_workspace_file(
    workspace_root: &Path,
    relative: &Path,
    expected_hex: &str,
) -> Result<HashVerification, HashVerifyError> {
    let expected = Sha256Digest::parse_hex(expected_hex)?;
    let actual = hash_workspace_file(workspace_root, relative)?;
    Ok(HashVerification {
        matches: actual.sha256 == expected,
        actual,
    })
}

fn resolve_existing_file(
    workspace_root: &Path,
    relative: &Path,
) -> Result<PathBuf, HashVerifyError> {
    validate_relative_path(relative)?;

    let canonical_root =
        fs::canonicalize(workspace_root).map_err(|_| HashVerifyError::Io)?;
    let root_metadata = fs::metadata(&canonical_root).map_err(|_| HashVerifyError::Io)?;
    if !root_metadata.is_dir() {
        return Err(HashVerifyError::InvalidWorkspacePath);
    }

    let mut candidate = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(HashVerifyError::InvalidWorkspacePath);
        };
        candidate.push(segment);
        let metadata = fs::symlink_metadata(&candidate).map_err(|_| HashVerifyError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(HashVerifyError::UnsafePath);
        }
    }

    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|_| HashVerifyError::Io)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(HashVerifyError::UnsafePath);
    }

    let metadata = fs::metadata(&canonical_candidate).map_err(|_| HashVerifyError::Io)?;
    if !metadata.is_file() {
        return Err(HashVerifyError::TargetNotRegularFile);
    }

    Ok(canonical_candidate)
}

fn validate_relative_path(relative: &Path) -> Result<(), HashVerifyError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(HashVerifyError::InvalidWorkspacePath);
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(HashVerifyError::InvalidWorkspacePath);
    }
    Ok(())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SHA256_EMPTY: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const SHA256_ABC: &str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA256_MILLION_A: &str =
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-hash-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("workspace");
        root
    }

    #[test]
    fn hashes_known_sha256_vectors() {
        let root = temporary_workspace("vectors");
        fs::write(root.join("empty.bin"), []).expect("empty");
        fs::write(root.join("abc.bin"), b"abc").expect("abc");
        assert_eq!(
            hash_workspace_file(&root, Path::new("empty.bin"))
                .expect("hash empty")
                .sha256
                .to_string(),
            SHA256_EMPTY
        );
        assert_eq!(
            hash_workspace_file(&root, Path::new("abc.bin"))
                .expect("hash abc")
                .sha256
                .to_string(),
            SHA256_ABC
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn streams_files_larger_than_the_read_buffer() {
        let root = temporary_workspace("streaming");
        fs::write(root.join("million-a.bin"), vec![b'a'; 1_000_000]).expect("seed");
        let result =
            hash_workspace_file(&root, Path::new("million-a.bin")).expect("hash large file");
        assert_eq!(result.bytes_hashed, 1_000_000);
        assert_eq!(result.sha256.to_string(), SHA256_MILLION_A);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn verification_reports_match_and_mismatch_without_writing() {
        let root = temporary_workspace("verify");
        let path = root.join("sample.bin");
        fs::write(&path, b"abc").expect("seed");
        let matching =
            verify_workspace_file(&root, Path::new("sample.bin"), SHA256_ABC).expect("verify");
        assert!(matching.matches);
        assert_eq!(matching.actual.bytes_hashed, 3);
        let mismatch =
            verify_workspace_file(&root, Path::new("sample.bin"), SHA256_EMPTY).expect("verify");
        assert!(!mismatch.matches);
        assert_eq!(mismatch.actual.sha256.to_string(), SHA256_ABC);
        assert_eq!(fs::read(&path).expect("read back"), b"abc");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn expected_digest_accepts_uppercase_and_surrounding_whitespace() {
        let parsed = Sha256Digest::parse_hex(
            "  BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD\n",
        )
        .expect("parse");
        assert_eq!(parsed.to_string(), SHA256_ABC);
    }

    #[test]
    fn rejects_malformed_expected_digest_before_hashing() {
        let root = temporary_workspace("invalid-expected");
        fs::write(root.join("sample.bin"), b"abc").expect("seed");
        assert_eq!(
            verify_workspace_file(&root, Path::new("sample.bin"), "abc").unwrap_err(),
            HashVerifyError::InvalidExpectedDigest
        );
        assert_eq!(
            verify_workspace_file(
                &root,
                Path::new("sample.bin"),
                "gg7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            )
            .unwrap_err(),
            HashVerifyError::InvalidExpectedDigest
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_parent_traversal_and_absolute_paths() {
        let root = temporary_workspace("path-boundary");
        fs::write(root.join("inside.bin"), b"inside").expect("inside");
        assert_eq!(
            hash_workspace_file(&root, Path::new("../outside.bin")).unwrap_err(),
            HashVerifyError::InvalidWorkspacePath
        );
        assert_eq!(
            hash_workspace_file(&root, &root.join("inside.bin")).unwrap_err(),
            HashVerifyError::InvalidWorkspacePath
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_directories() {
        let root = temporary_workspace("directory");
        fs::create_dir(root.join("folder")).expect("folder");
        assert_eq!(
            hash_workspace_file(&root, Path::new("folder")).unwrap_err(),
            HashVerifyError::TargetNotRegularFile
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
        symlink(&outside, root.join("link.bin")).expect("symlink");
        assert_eq!(
            hash_workspace_file(&root, Path::new("link.bin")).unwrap_err(),
            HashVerifyError::UnsafePath
        );
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_file(outside).expect("cleanup outside");
    }
}
