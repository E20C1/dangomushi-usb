use crate::recovery_snapshot::{self, RecoverySnapshotSummary};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const REBUILD_MARKER: &str = ".dangomushi-frozen-rebuild";
pub const REBUILD_FORMAT_VERSION: u16 = 1;
pub const PINNED_RUST_TOOLCHAIN: &str = "1.99.1";
pub const MAX_REBUILD_BINARY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const REBUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

const IO_BUFFER_BYTES: usize = 64 * 1024;
const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";
const VENDOR_CONFIG_FILE: &str = ".cargo/vendor-config.toml";
const EXPECTED_TOOLCHAIN: &str =
    "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\n";
const EXPECTED_VENDOR_CONFIG: &str = "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"\n\n[net]\noffline = true\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuiltArtifact {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

impl RebuiltArtifact {
    pub fn sha256_hex(&self) -> String {
        hex_lower(&self.sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenRebuildSummary {
    pub source_snapshot: RecoverySnapshotSummary,
    pub toolchain: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub cli: RebuiltArtifact,
    pub desktop: RebuiltArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrozenRebuildError {
    StagingUnavailable,
    StagingVerificationFailed,
    UnsupportedToolchain,
    InvalidVendorConfig,
    AmbientCargoConfig,
    DestinationExists,
    DestinationInsideProtectedRoot,
    UnsafeDestinationParent,
    CargoUnavailable,
    CargoFailed,
    CargoTimedOut,
    BuildArtifactMissing,
    UnsafeBuildArtifact,
    BuildArtifactTooLarge,
    StagingChanged,
    Io,
}

impl fmt::Display for FrozenRebuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::StagingUnavailable => "verified reconstruction staging root is unavailable",
            Self::StagingVerificationFailed => {
                "reconstruction staging root failed Recovery Snapshot verification"
            }
            Self::UnsupportedToolchain => "staged Rust toolchain pin is not the supported rebuild baseline",
            Self::InvalidVendorConfig => "staged Cargo vendor configuration is not the required offline source replacement",
            Self::AmbientCargoConfig => "an ambient Cargo config outside the staged source would affect the rebuild",
            Self::DestinationExists => "frozen rebuild destination already exists",
            Self::DestinationInsideProtectedRoot => {
                "frozen rebuild destination must remain outside staged/live protected roots"
            }
            Self::UnsafeDestinationParent => "frozen rebuild destination parent is unsafe",
            Self::CargoUnavailable => "pinned Cargo toolchain is unavailable",
            Self::CargoFailed => "frozen Cargo rebuild failed",
            Self::CargoTimedOut => "frozen Cargo rebuild exceeded its time limit",
            Self::BuildArtifactMissing => "expected rebuilt application artifact is missing",
            Self::UnsafeBuildArtifact => "rebuilt application artifact is not a real regular file",
            Self::BuildArtifactTooLarge => "rebuilt application artifact exceeded the size limit",
            Self::StagingChanged => "staged recovery source changed during the rebuild",
            Self::Io => "frozen rebuild I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for FrozenRebuildError {}

pub fn frozen_rebuild(
    staged_root: &Path,
    destination: &Path,
    protected_source_root: Option<&Path>,
    protected_workspace_root: Option<&Path>,
) -> Result<FrozenRebuildSummary, FrozenRebuildError> {
    // No destination mutation is allowed until the staged Recovery Snapshot
    // passes the existing complete verification boundary.
    let before = recovery_snapshot::verify_snapshot(staged_root)
        .map_err(|_| FrozenRebuildError::StagingVerificationFailed)?;

    let canonical_staging =
        fs::canonicalize(staged_root).map_err(|_| FrozenRebuildError::StagingUnavailable)?;
    if !canonical_staging.is_dir() {
        return Err(FrozenRebuildError::StagingUnavailable);
    }

    validate_fixed_rebuild_inputs(&canonical_staging)?;
    reject_ambient_cargo_configs(&canonical_staging)?;

    let protected_source = canonical_optional_root(protected_source_root)?;
    let protected_workspace = canonical_optional_root(protected_workspace_root)?;
    let mut protected = vec![canonical_staging.as_path()];
    if let Some(path) = protected_source.as_deref() {
        protected.push(path);
    }
    if let Some(path) = protected_workspace.as_deref() {
        protected.push(path);
    }

    let (destination_parent, destination_path) =
        checked_new_destination(destination, &protected)?;

    fs::create_dir(&destination_path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            FrozenRebuildError::DestinationExists
        } else {
            FrozenRebuildError::Io
        }
    })?;

    let result = rebuild_into_new_directory(&canonical_staging, &destination_path, before);

    match result {
        Ok(summary) => Ok(summary),
        Err(error) => {
            let _ = fs::remove_dir_all(&destination_path);
            let _ = sync_directory(&destination_parent);
            Err(error)
        }
    }
}

fn rebuild_into_new_directory(
    staged_root: &Path,
    destination: &Path,
    before: RecoverySnapshotSummary,
) -> Result<FrozenRebuildSummary, FrozenRebuildError> {
    let target_dir = destination.join("target");
    let cargo_home = destination.join(".cargo-home");
    fs::create_dir(&target_dir).map_err(|_| FrozenRebuildError::Io)?;
    fs::create_dir(&cargo_home).map_err(|_| FrozenRebuildError::Io)?;

    run_cargo_build(
        staged_root,
        &target_dir,
        &cargo_home,
        &["--bin", "dangomushi"],
    )?;
    run_cargo_build(
        staged_root,
        &target_dir,
        &cargo_home,
        &["--features", "desktop", "--bin", "dangomushi-desktop"],
    )?;

    // Cargo/build scripts are allowed to execute for this task, but the
    // authenticated staged source is not allowed to mutate as a side effect.
    let after = recovery_snapshot::verify_snapshot(staged_root)
        .map_err(|_| FrozenRebuildError::StagingChanged)?;
    if after != before {
        return Err(FrozenRebuildError::StagingChanged);
    }

    let release_dir = target_dir.join("release");
    let cli_name = executable_name("dangomushi");
    let desktop_name = executable_name("dangomushi-desktop");
    let cli_path = release_dir.join(&cli_name);
    let desktop_path = release_dir.join(&desktop_name);

    let cli = hash_artifact(
        &cli_path,
        &format!("target/release/{cli_name}"),
    )?;
    let desktop = hash_artifact(
        &desktop_path,
        &format!("target/release/{desktop_name}"),
    )?;

    // Cargo home is only a clean configuration/cache isolation surface for the
    // rebuild process. It is not part of accepted rebuilt application output.
    fs::remove_dir_all(&cargo_home).map_err(|_| FrozenRebuildError::Io)?;

    let summary = FrozenRebuildSummary {
        source_snapshot: before,
        toolchain: PINNED_RUST_TOOLCHAIN,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        cli,
        desktop,
    };

    write_rebuild_marker(destination, &summary)?;
    sync_directory(destination)?;
    if let Some(parent) = destination.parent() {
        sync_directory(parent)?;
    }

    Ok(summary)
}

fn run_cargo_build(
    staged_root: &Path,
    target_dir: &Path,
    cargo_home: &Path,
    build_args: &[&str],
) -> Result<(), FrozenRebuildError> {
    let mut command = Command::new("cargo");
    command
        .current_dir(staged_root)
        .arg(format!("+{PINNED_RUST_TOOLCHAIN}"))
        .arg("--config")
        .arg(VENDOR_CONFIG_FILE)
        .arg("build")
        .arg("--frozen")
        .arg("--release")
        .arg("--manifest-path")
        .arg("Cargo.toml")
        .arg("--target-dir")
        .arg(target_dir);

    for arg in build_args {
        command.arg(arg);
    }

    sanitize_cargo_environment(&mut command, cargo_home);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut child = command
        .spawn()
        .map_err(|_| FrozenRebuildError::CargoUnavailable)?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(FrozenRebuildError::CargoFailed),
            Ok(None) => {
                if started.elapsed() >= REBUILD_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(FrozenRebuildError::CargoTimedOut);
                }
                thread::sleep(Duration::from_millis(200));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FrozenRebuildError::CargoFailed);
            }
        }
    }
}

fn sanitize_cargo_environment(command: &mut Command, cargo_home: &Path) {
    for (key, _) in std::env::vars_os() {
        let text = key.to_string_lossy();
        if text.starts_with("CARGO_")
            || matches!(
                text.as_ref(),
                "CARGO"
                    | "RUSTC"
                    | "RUSTC_WRAPPER"
                    | "RUSTC_WORKSPACE_WRAPPER"
                    | "RUSTDOC"
                    | "RUSTFLAGS"
                    | "RUSTDOCFLAGS"
                    | "RUSTUP_TOOLCHAIN"
            )
        {
            command.env_remove(&key);
        }
    }

    command.env("CARGO_HOME", cargo_home);
    command.env("CARGO_NET_OFFLINE", "true");
}

fn validate_fixed_rebuild_inputs(staged_root: &Path) -> Result<(), FrozenRebuildError> {
    let toolchain = read_small_regular_file(&staged_root.join(TOOLCHAIN_FILE), 64 * 1024)?;
    if normalize_newlines(&toolchain) != EXPECTED_TOOLCHAIN {
        return Err(FrozenRebuildError::UnsupportedToolchain);
    }

    let vendor_config =
        read_small_regular_file(&staged_root.join(VENDOR_CONFIG_FILE), 64 * 1024)?;
    if normalize_newlines(&vendor_config) != EXPECTED_VENDOR_CONFIG {
        return Err(FrozenRebuildError::InvalidVendorConfig);
    }

    Ok(())
}

fn read_small_regular_file(path: &Path, limit: usize) -> Result<String, FrozenRebuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FrozenRebuildError::StagingUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(FrozenRebuildError::StagingUnavailable);
    }
    if metadata.len() > limit as u64 {
        return Err(FrozenRebuildError::StagingUnavailable);
    }

    let mut file = File::open(path).map_err(|_| FrozenRebuildError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FrozenRebuildError::Io)?;
    if bytes.len() > limit {
        return Err(FrozenRebuildError::StagingUnavailable);
    }
    String::from_utf8(bytes).map_err(|_| FrozenRebuildError::StagingUnavailable)
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn reject_ambient_cargo_configs(staged_root: &Path) -> Result<(), FrozenRebuildError> {
    let mut current = staged_root.parent();
    while let Some(directory) = current {
        for name in ["config", "config.toml"] {
            let candidate = directory.join(".cargo").join(name);
            match fs::symlink_metadata(&candidate) {
                Ok(_) => return Err(FrozenRebuildError::AmbientCargoConfig),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(FrozenRebuildError::Io),
            }
        }
        current = directory.parent();
    }
    Ok(())
}

fn canonical_optional_root(path: Option<&Path>) -> Result<Option<PathBuf>, FrozenRebuildError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let metadata =
        fs::symlink_metadata(path).map_err(|_| FrozenRebuildError::UnsafeDestinationParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FrozenRebuildError::UnsafeDestinationParent);
    }
    let canonical =
        fs::canonicalize(path).map_err(|_| FrozenRebuildError::UnsafeDestinationParent)?;
    Ok(Some(canonical))
}

fn checked_new_destination(
    destination: &Path,
    protected_roots: &[&Path],
) -> Result<(PathBuf, PathBuf), FrozenRebuildError> {
    if destination.as_os_str().is_empty() {
        return Err(FrozenRebuildError::UnsafeDestinationParent);
    }

    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(FrozenRebuildError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FrozenRebuildError::Io),
    }

    let name = destination
        .file_name()
        .ok_or(FrozenRebuildError::UnsafeDestinationParent)?;
    if name == OsStr::new(".") || name == OsStr::new("..") {
        return Err(FrozenRebuildError::UnsafeDestinationParent);
    }

    let parent = match destination.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let metadata =
        fs::symlink_metadata(parent).map_err(|_| FrozenRebuildError::UnsafeDestinationParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FrozenRebuildError::UnsafeDestinationParent);
    }

    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| FrozenRebuildError::UnsafeDestinationParent)?;
    let final_path = canonical_parent.join(name);

    if protected_roots
        .iter()
        .any(|root| final_path.starts_with(root) || root.starts_with(&final_path))
    {
        return Err(FrozenRebuildError::DestinationInsideProtectedRoot);
    }

    Ok((canonical_parent, final_path))
}

fn hash_artifact(path: &Path, relative_path: &str) -> Result<RebuiltArtifact, FrozenRebuildError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FrozenRebuildError::BuildArtifactMissing)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(FrozenRebuildError::UnsafeBuildArtifact);
    }
    if metadata.len() > MAX_REBUILD_BINARY_BYTES {
        return Err(FrozenRebuildError::BuildArtifactTooLarge);
    }

    let mut file = File::open(path).map_err(|_| FrozenRebuildError::Io)?;
    let mut buffer = [0u8; IO_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;

    loop {
        let count = file.read(&mut buffer).map_err(|_| FrozenRebuildError::Io)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or(FrozenRebuildError::BuildArtifactTooLarge)?;
        if bytes > MAX_REBUILD_BINARY_BYTES {
            return Err(FrozenRebuildError::BuildArtifactTooLarge);
        }
        hasher.update(&buffer[..count]);
    }

    if bytes != metadata.len() {
        return Err(FrozenRebuildError::UnsafeBuildArtifact);
    }

    Ok(RebuiltArtifact {
        relative_path: relative_path.to_string(),
        bytes,
        sha256: hasher.finalize().into(),
    })
}

fn write_rebuild_marker(
    destination: &Path,
    summary: &FrozenRebuildSummary,
) -> Result<(), FrozenRebuildError> {
    let marker = format!(
        "DANGOMUSHI_FROZEN_REBUILD_V1\nkind=dangomushi-frozen-rebuild\nformat={}\nrunner_version={}\nsource_snapshot_sha256={}\ntoolchain={}\nos={}\narch={}\ncli={}|{}|{}\ndesktop={}|{}|{}\n",
        REBUILD_FORMAT_VERSION,
        env!("CARGO_PKG_VERSION"),
        summary.source_snapshot.sha256_hex(),
        summary.toolchain,
        summary.os,
        summary.arch,
        summary.cli.relative_path,
        summary.cli.bytes,
        summary.cli.sha256_hex(),
        summary.desktop.relative_path,
        summary.desktop.bytes,
        summary.desktop.sha256_hex(),
    );

    let marker_path = destination.join(REBUILD_MARKER);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker_path)
        .map_err(|_| FrozenRebuildError::Io)?;
    file.write_all(marker.as_bytes())
        .map_err(|_| FrozenRebuildError::Io)?;
    file.sync_all().map_err(|_| FrozenRebuildError::Io)?;
    Ok(())
}

fn executable_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), FrozenRebuildError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FrozenRebuildError::Io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), FrozenRebuildError> {
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

    #[test]
    fn newline_normalization_is_cross_platform() {
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn executable_names_follow_host() {
        let cli = executable_name("dangomushi");
        if cfg!(windows) {
            assert_eq!(cli, "dangomushi.exe");
        } else {
            assert_eq!(cli, "dangomushi");
        }
    }
}
