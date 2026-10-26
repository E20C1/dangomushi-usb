use crate::recovery_snapshot::{self, RecoverySnapshotSummary};
use crate::workspace;
use crate::workspace_backup::{self, WorkspaceBackupSummary};
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
pub const SMOKE_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_SMOKE_OUTPUT_BYTES: u64 = 64 * 1024;
pub const MAX_REBUILD_MARKER_BYTES: u64 = 64 * 1024;

const IO_BUFFER_BYTES: usize = 64 * 1024;
const SMOKE_SANDBOX_DIR: &str = ".dangomushi-recovery-smoke";
const REBUILD_MARKER_MAGIC: &str = "DANGOMUSHI_FROZEN_REBUILD_V1";
const REBUILD_MARKER_KIND: &str = "dangomushi-frozen-rebuild";
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


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredSmokeSummary {
    pub source_snapshot: RecoverySnapshotSummary,
    pub rebuild_runner_version: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub cli: RebuiltArtifact,
    pub desktop: RebuiltArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveredSmokeError {
    StagingVerificationFailed,
    UnsafeRebuildRoot,
    RebuildMarkerMissing,
    InvalidRebuildMarker,
    PlatformMismatch,
    SourceSnapshotMismatch,
    InvalidStagedManifest,
    ArtifactInvalid,
    ArtifactMismatch,
    SmokeStateExists,
    ProcessLaunchFailed,
    ProcessTimedOut,
    ProcessFailed,
    UnexpectedOutput,
    RebuildChanged,
    StagingChanged,
    Io,
}

impl fmt::Display for RecoveredSmokeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::StagingVerificationFailed => {
                "staged Recovery Snapshot failed verification before smoke execution"
            }
            Self::UnsafeRebuildRoot => "rebuild root is unsafe or overlaps verified staging",
            Self::RebuildMarkerMissing => "frozen rebuild completion marker is missing",
            Self::InvalidRebuildMarker => "frozen rebuild completion marker is invalid",
            Self::PlatformMismatch => "rebuilt artifacts do not target the current OS/architecture",
            Self::SourceSnapshotMismatch => {
                "frozen rebuild marker does not match the verified staged snapshot"
            }
            Self::InvalidStagedManifest => "staged Cargo.toml package version is invalid",
            Self::ArtifactInvalid => "recovered application artifact is missing or unsafe",
            Self::ArtifactMismatch => {
                "recovered application artifact does not match rebuild integrity metadata"
            }
            Self::SmokeStateExists => "recovery smoke sandbox already exists",
            Self::ProcessLaunchFailed => "recovered application smoke process could not be launched",
            Self::ProcessTimedOut => "recovered application smoke process timed out",
            Self::ProcessFailed => "recovered application smoke process returned failure",
            Self::UnexpectedOutput => "recovered application smoke output was unexpected",
            Self::RebuildChanged => "rebuild marker or recovered artifacts changed during smoke execution",
            Self::StagingChanged => "verified staging changed during smoke execution",
            Self::Io => "recovered application smoke verification I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for RecoveredSmokeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRebuildMarker {
    runner_version: String,
    source_snapshot_sha256: [u8; 32],
    toolchain: String,
    os: String,
    arch: String,
    cli: RebuiltArtifact,
    desktop: RebuiltArtifact,
}

pub fn smoke_verify_rebuild(
    staged_root: &Path,
    rebuild_root: &Path,
) -> Result<RecoveredSmokeSummary, RecoveredSmokeError> {
    let before_staging = recovery_snapshot::verify_snapshot(staged_root)
        .map_err(|_| RecoveredSmokeError::StagingVerificationFailed)?;

    let staged_metadata =
        fs::symlink_metadata(staged_root).map_err(|_| RecoveredSmokeError::StagingVerificationFailed)?;
    if staged_metadata.file_type().is_symlink() || !staged_metadata.is_dir() {
        return Err(RecoveredSmokeError::StagingVerificationFailed);
    }
    let canonical_staging =
        fs::canonicalize(staged_root).map_err(|_| RecoveredSmokeError::StagingVerificationFailed)?;

    let rebuild_metadata =
        fs::symlink_metadata(rebuild_root).map_err(|_| RecoveredSmokeError::UnsafeRebuildRoot)?;
    if rebuild_metadata.file_type().is_symlink() || !rebuild_metadata.is_dir() {
        return Err(RecoveredSmokeError::UnsafeRebuildRoot);
    }
    let canonical_rebuild =
        fs::canonicalize(rebuild_root).map_err(|_| RecoveredSmokeError::UnsafeRebuildRoot)?;
    if canonical_rebuild.starts_with(&canonical_staging)
        || canonical_staging.starts_with(&canonical_rebuild)
    {
        return Err(RecoveredSmokeError::UnsafeRebuildRoot);
    }

    let marker_path = canonical_rebuild.join(REBUILD_MARKER);
    let marker_before = read_bounded_regular(
        &marker_path,
        MAX_REBUILD_MARKER_BYTES,
        RecoveredSmokeError::RebuildMarkerMissing,
    )?;
    let marker = parse_rebuild_marker(&marker_before)?;

    if marker.toolchain != PINNED_RUST_TOOLCHAIN
        || marker.os != std::env::consts::OS
        || marker.arch != std::env::consts::ARCH
    {
        return Err(RecoveredSmokeError::PlatformMismatch);
    }
    if marker.source_snapshot_sha256 != before_staging.sha256 {
        return Err(RecoveredSmokeError::SourceSnapshotMismatch);
    }

    let expected_cli_path = format!("target/release/{}", executable_name("dangomushi"));
    let expected_desktop_path =
        format!("target/release/{}", executable_name("dangomushi-desktop"));
    if marker.cli.relative_path != expected_cli_path
        || marker.desktop.relative_path != expected_desktop_path
    {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }

    verify_recorded_artifact(&canonical_rebuild, &marker.cli)?;
    verify_recorded_artifact(&canonical_rebuild, &marker.desktop)?;
    let app_version = staged_package_version(&canonical_staging)?;

    let sandbox = canonical_rebuild.join(SMOKE_SANDBOX_DIR);
    match fs::symlink_metadata(&sandbox) {
        Ok(_) => return Err(RecoveredSmokeError::SmokeStateExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(RecoveredSmokeError::Io),
    }
    fs::create_dir(&sandbox).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            RecoveredSmokeError::SmokeStateExists
        } else {
            RecoveredSmokeError::Io
        }
    })?;

    let smoke_result = (|| {
        let cli_path = canonical_rebuild.join(&marker.cli.relative_path);
        let desktop_path = canonical_rebuild.join(&marker.desktop.relative_path);

        let cli_stdout = run_smoke_command(
            &cli_path,
            &["version"],
            &sandbox,
            "cli",
        )?;
        let expected_cli = format!("dangomushi {app_version}\n");
        if normalize_newlines(&cli_stdout) != expected_cli {
            return Err(RecoveredSmokeError::UnexpectedOutput);
        }

        let desktop_stdout = run_smoke_command(
            &desktop_path,
            &["--recovery-smoke-check"],
            &sandbox,
            "desktop",
        )?;
        let expected_desktop =
            format!("dangomushi-desktop {app_version} recovery-smoke-ok\n");
        if normalize_newlines(&desktop_stdout) != expected_desktop {
            return Err(RecoveredSmokeError::UnexpectedOutput);
        }

        Ok(())
    })();

    let cleanup_result = fs::remove_dir_all(&sandbox).map_err(|_| RecoveredSmokeError::Io);
    if let Err(error) = smoke_result {
        let _ = cleanup_result;
        return Err(error);
    }
    cleanup_result?;

    let marker_after = read_bounded_regular(
        &marker_path,
        MAX_REBUILD_MARKER_BYTES,
        RecoveredSmokeError::RebuildChanged,
    )?;
    if marker_after != marker_before {
        return Err(RecoveredSmokeError::RebuildChanged);
    }

    verify_recorded_artifact(&canonical_rebuild, &marker.cli)
        .map_err(|_| RecoveredSmokeError::RebuildChanged)?;
    verify_recorded_artifact(&canonical_rebuild, &marker.desktop)
        .map_err(|_| RecoveredSmokeError::RebuildChanged)?;

    let after_staging = recovery_snapshot::verify_snapshot(&canonical_staging)
        .map_err(|_| RecoveredSmokeError::StagingChanged)?;
    if after_staging != before_staging {
        return Err(RecoveredSmokeError::StagingChanged);
    }

    Ok(RecoveredSmokeSummary {
        source_snapshot: before_staging,
        rebuild_runner_version: marker.runner_version,
        app_version,
        os: marker.os,
        arch: marker.arch,
        cli: marker.cli,
        desktop: marker.desktop,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshWorkspaceRestoreSummary {
    pub source_snapshot: RecoverySnapshotSummary,
    pub app_version: String,
    pub workspace_backup: WorkspaceBackupSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshWorkspaceRestoreError {
    StagingUnavailable,
    RebuildUnavailable,
    SmokeVerificationFailed,
    WorkspaceBackupUnavailable,
    WorkspaceBackupMismatch,
    DestinationExists,
    DestinationInsideProtectedRoot,
    UnsafeDestinationParent,
    RestoreFailed,
    StagingChanged,
    Io,
}

impl fmt::Display for FreshWorkspaceRestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::StagingUnavailable => "verified reconstruction staging root is unavailable",
            Self::RebuildUnavailable => "verified reconstruction rebuild root is unavailable",
            Self::SmokeVerificationFailed => {
                "recovered application smoke verification failed before Workspace restore"
            }
            Self::WorkspaceBackupUnavailable => {
                "staged Generic Workspace Backup is unavailable or invalid"
            }
            Self::WorkspaceBackupMismatch => {
                "staged Generic Workspace Backup does not match the verified Recovery Snapshot"
            }
            Self::DestinationExists => "fresh Workspace restore destination already exists",
            Self::DestinationInsideProtectedRoot => {
                "fresh Workspace restore destination overlaps a protected reconstruction/live root"
            }
            Self::UnsafeDestinationParent => "fresh Workspace restore destination parent is unsafe",
            Self::RestoreFailed => "fresh Workspace restore failed",
            Self::StagingChanged => "verified reconstruction staging changed during Workspace restore",
            Self::Io => "fresh Workspace restore I/O failed",
        };
        formatter.write_str(message)
    }
}

impl Error for FreshWorkspaceRestoreError {}

pub fn restore_workspace_into_fresh_destination(
    staged_root: &Path,
    rebuild_root: &Path,
    destination: &Path,
    protected_source_root: Option<&Path>,
    protected_workspace_root: Option<&Path>,
) -> Result<FreshWorkspaceRestoreSummary, FreshWorkspaceRestoreError> {
    let canonical_staging =
        checked_restore_root(staged_root, FreshWorkspaceRestoreError::StagingUnavailable)?;
    let canonical_rebuild =
        checked_restore_root(rebuild_root, FreshWorkspaceRestoreError::RebuildUnavailable)?;
    if canonical_rebuild.starts_with(&canonical_staging)
        || canonical_staging.starts_with(&canonical_rebuild)
    {
        return Err(FreshWorkspaceRestoreError::DestinationInsideProtectedRoot);
    }

    let protected_source = checked_optional_restore_root(protected_source_root)?;
    let protected_workspace = checked_optional_restore_root(protected_workspace_root)?;
    let mut protected = vec![canonical_staging.as_path(), canonical_rebuild.as_path()];
    if let Some(path) = protected_source.as_deref() {
        protected.push(path);
    }
    if let Some(path) = protected_workspace.as_deref() {
        protected.push(path);
    }

    // Destination safety is checked before smoke execution, but no destination
    // path is created or mutated until the complete smoke gate has succeeded.
    let destination_path = checked_fresh_restore_destination(destination, &protected)?;

    let smoke = smoke_verify_rebuild(&canonical_staging, &canonical_rebuild)
        .map_err(|_| FreshWorkspaceRestoreError::SmokeVerificationFailed)?;

    let backup_path = canonical_staging.join("recovery").join("workspace.dgmwbk");
    let backup_before = workspace_backup::verify_backup_file(&backup_path)
        .map_err(|_| FreshWorkspaceRestoreError::WorkspaceBackupUnavailable)?;
    if backup_before != smoke.source_snapshot.workspace_backup {
        return Err(FreshWorkspaceRestoreError::WorkspaceBackupMismatch);
    }

    let restored = workspace_backup::restore_backup_file_into_fresh_workspace(
        &backup_path,
        &destination_path,
    )
    .map_err(|_| FreshWorkspaceRestoreError::RestoreFailed)?;

    if restored != backup_before {
        let _ = fs::remove_dir_all(&destination_path);
        return Err(FreshWorkspaceRestoreError::WorkspaceBackupMismatch);
    }

    let after_staging = match recovery_snapshot::verify_snapshot(&canonical_staging) {
        Ok(summary) => summary,
        Err(_) => {
            let _ = fs::remove_dir_all(&destination_path);
            return Err(FreshWorkspaceRestoreError::StagingChanged);
        }
    };
    let backup_after = match workspace_backup::verify_backup_file(&backup_path) {
        Ok(summary) => summary,
        Err(_) => {
            let _ = fs::remove_dir_all(&destination_path);
            return Err(FreshWorkspaceRestoreError::StagingChanged);
        }
    };
    if after_staging != smoke.source_snapshot || backup_after != backup_before {
        let _ = fs::remove_dir_all(&destination_path);
        return Err(FreshWorkspaceRestoreError::StagingChanged);
    }

    Ok(FreshWorkspaceRestoreSummary {
        source_snapshot: smoke.source_snapshot,
        app_version: smoke.app_version,
        workspace_backup: restored,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructionVerificationSummary {
    pub source_snapshot: RecoverySnapshotSummary,
    pub rebuild_runner_version: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub cli: RebuiltArtifact,
    pub desktop: RebuiltArtifact,
    pub workspace_backup: WorkspaceBackupSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructionVerificationError {
    StagingUnavailable,
    RebuildUnavailable,
    WorkspaceUnavailable,
    EvidenceRootsOverlap,
    SmokeVerificationFailed,
    WorkspaceBackupUnavailable,
    WorkspaceBackupMismatch,
    WorkspaceContentsMismatch,
    WorkspaceLayoutMismatch,
    StagingChanged,
}

impl fmt::Display for ReconstructionVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::StagingUnavailable => "verified reconstruction staging root is unavailable",
            Self::RebuildUnavailable => "verified reconstruction rebuild root is unavailable",
            Self::WorkspaceUnavailable => "restored reconstruction Workspace is unavailable",
            Self::EvidenceRootsOverlap => {
                "staging, rebuild and restored Workspace evidence roots must remain separate"
            }
            Self::SmokeVerificationFailed => {
                "recovered application smoke evidence failed reconstruction verification"
            }
            Self::WorkspaceBackupUnavailable => {
                "staged Generic Workspace Backup is unavailable or invalid"
            }
            Self::WorkspaceBackupMismatch => {
                "staged Generic Workspace Backup does not match the verified Recovery Snapshot"
            }
            Self::WorkspaceContentsMismatch => {
                "restored Workspace managed contents do not match the authenticated Generic Backup"
            }
            Self::WorkspaceLayoutMismatch => {
                "restored Workspace is not the expected fresh reconstruction layout"
            }
            Self::StagingChanged => {
                "verified reconstruction staging changed during final verification"
            }
        };
        formatter.write_str(message)
    }
}

impl Error for ReconstructionVerificationError {}

pub fn verify_reconstruction(
    staged_root: &Path,
    rebuild_root: &Path,
    workspace_root: &Path,
) -> Result<ReconstructionVerificationSummary, ReconstructionVerificationError> {
    let canonical_staging = checked_verification_root(
        staged_root,
        ReconstructionVerificationError::StagingUnavailable,
    )?;
    let canonical_rebuild = checked_verification_root(
        rebuild_root,
        ReconstructionVerificationError::RebuildUnavailable,
    )?;
    let canonical_workspace = checked_verification_root(
        workspace_root,
        ReconstructionVerificationError::WorkspaceUnavailable,
    )?;

    if roots_overlap(&canonical_staging, &canonical_rebuild)
        || roots_overlap(&canonical_staging, &canonical_workspace)
        || roots_overlap(&canonical_rebuild, &canonical_workspace)
    {
        return Err(ReconstructionVerificationError::EvidenceRootsOverlap);
    }

    verify_fresh_reconstruction_layout(&canonical_workspace)?;

    // Reuse the existing bounded execution gate rather than defining a second
    // executable-verification path. This re-verifies staging, rebuild metadata,
    // artifact hashes and the two fixed smoke entrypoints.
    let smoke = smoke_verify_rebuild(&canonical_staging, &canonical_rebuild)
        .map_err(|_| ReconstructionVerificationError::SmokeVerificationFailed)?;

    let backup_path = canonical_staging.join("recovery").join("workspace.dgmwbk");
    let backup = workspace_backup::verify_backup_file(&backup_path)
        .map_err(|_| ReconstructionVerificationError::WorkspaceBackupUnavailable)?;
    if backup != smoke.source_snapshot.workspace_backup {
        return Err(ReconstructionVerificationError::WorkspaceBackupMismatch);
    }

    let matched = workspace_backup::verify_backup_file_matches_workspace(
        &backup_path,
        &canonical_workspace,
    )
    .map_err(|_| ReconstructionVerificationError::WorkspaceContentsMismatch)?;
    if matched != backup {
        return Err(ReconstructionVerificationError::WorkspaceBackupMismatch);
    }

    // Smoke execution is not an OS sandbox. Verify the fresh Workspace layout
    // and exact managed contents after that executable-code authority has run.
    verify_fresh_reconstruction_layout(&canonical_workspace)?;
    let matched_after = workspace_backup::verify_backup_file_matches_workspace(
        &backup_path,
        &canonical_workspace,
    )
    .map_err(|_| ReconstructionVerificationError::WorkspaceContentsMismatch)?;
    if matched_after != backup {
        return Err(ReconstructionVerificationError::WorkspaceBackupMismatch);
    }

    let after_staging = recovery_snapshot::verify_snapshot(&canonical_staging)
        .map_err(|_| ReconstructionVerificationError::StagingChanged)?;
    if after_staging != smoke.source_snapshot {
        return Err(ReconstructionVerificationError::StagingChanged);
    }

    Ok(ReconstructionVerificationSummary {
        source_snapshot: smoke.source_snapshot,
        rebuild_runner_version: smoke.rebuild_runner_version,
        app_version: smoke.app_version,
        os: smoke.os,
        arch: smoke.arch,
        cli: smoke.cli,
        desktop: smoke.desktop,
        workspace_backup: matched_after,
    })
}

fn checked_verification_root(
    path: &Path,
    error: ReconstructionVerificationError,
) -> Result<PathBuf, ReconstructionVerificationError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error);
    }
    fs::canonicalize(path).map_err(|_| error)
}

fn roots_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn verify_fresh_reconstruction_layout(
    workspace_root: &Path,
) -> Result<(), ReconstructionVerificationError> {
    if !workspace::doctor(workspace_root).ok() {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }

    let expected_top = [
        (".dangomushi-workspace", false),
        ("backups", true),
        ("config", true),
        ("data", true),
        ("identity", true),
        ("repositories", true),
        ("state", true),
    ];
    let mut observed_top = Vec::<(String, bool)>::new();
    for entry in fs::read_dir(workspace_root)
        .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?
    {
        let entry = entry.map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ReconstructionVerificationError::WorkspaceLayoutMismatch)?
            .to_string();
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?;
        if metadata.file_type().is_symlink() {
            return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
        }
        observed_top.push((name, metadata.is_dir()));
    }
    observed_top.sort_by(|left, right| left.0.cmp(&right.0));
    if observed_top != expected_top {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }

    let backups = workspace_root.join("backups");
    if fs::read_dir(&backups)
        .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?
        .next()
        .is_some()
    {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }

    let state = workspace_root.join("state");
    let mut state_entries = fs::read_dir(&state)
        .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?;
    if state_entries.len() != 1 {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }
    let autosave = state_entries.pop().expect("one state entry");
    if autosave.file_name() != OsStr::new("autosave") {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }
    let autosave_metadata = fs::symlink_metadata(autosave.path())
        .map_err(|_| ReconstructionVerificationError::WorkspaceLayoutMismatch)?;
    if autosave_metadata.file_type().is_symlink() || !autosave_metadata.is_dir() {
        return Err(ReconstructionVerificationError::WorkspaceLayoutMismatch);
    }

    Ok(())
}

fn checked_restore_root(
    path: &Path,
    error: FreshWorkspaceRestoreError,
) -> Result<PathBuf, FreshWorkspaceRestoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error);
    }
    fs::canonicalize(path).map_err(|_| error)
}

fn checked_optional_restore_root(
    path: Option<&Path>,
) -> Result<Option<PathBuf>, FreshWorkspaceRestoreError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let metadata =
        fs::symlink_metadata(path).map_err(|_| FreshWorkspaceRestoreError::UnsafeDestinationParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FreshWorkspaceRestoreError::UnsafeDestinationParent);
    }
    fs::canonicalize(path)
        .map(Some)
        .map_err(|_| FreshWorkspaceRestoreError::UnsafeDestinationParent)
}

fn checked_fresh_restore_destination(
    destination: &Path,
    protected_roots: &[&Path],
) -> Result<PathBuf, FreshWorkspaceRestoreError> {
    if destination.as_os_str().is_empty() {
        return Err(FreshWorkspaceRestoreError::UnsafeDestinationParent);
    }

    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(FreshWorkspaceRestoreError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(FreshWorkspaceRestoreError::Io),
    }

    let name = destination
        .file_name()
        .ok_or(FreshWorkspaceRestoreError::UnsafeDestinationParent)?;
    if name == OsStr::new(".") || name == OsStr::new("..") {
        return Err(FreshWorkspaceRestoreError::UnsafeDestinationParent);
    }

    let parent = match destination.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let metadata = fs::symlink_metadata(parent)
        .map_err(|_| FreshWorkspaceRestoreError::UnsafeDestinationParent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(FreshWorkspaceRestoreError::UnsafeDestinationParent);
    }

    let canonical_parent = fs::canonicalize(parent)
        .map_err(|_| FreshWorkspaceRestoreError::UnsafeDestinationParent)?;
    let final_path = canonical_parent.join(name);

    if protected_roots
        .iter()
        .any(|root| final_path.starts_with(root) || root.starts_with(&final_path))
    {
        return Err(FreshWorkspaceRestoreError::DestinationInsideProtectedRoot);
    }

    Ok(final_path)
}

fn parse_rebuild_marker(bytes: &[u8]) -> Result<ParsedRebuildMarker, RecoveredSmokeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RecoveredSmokeError::InvalidRebuildMarker)?;
    let normalized = normalize_newlines(text);
    if !normalized.ends_with('\n') {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }
    let lines: Vec<&str> = normalized.trim_end_matches('\n').split('\n').collect();
    if lines.len() != 10
        || lines[0] != REBUILD_MARKER_MAGIC
        || lines[1] != format!("kind={REBUILD_MARKER_KIND}")
        || lines[2] != format!("format={REBUILD_FORMAT_VERSION}")
    {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }

    let runner_version = parse_marker_string(lines[3], "runner_version=")?;
    let source_snapshot_sha256 =
        parse_lower_hex_32(parse_marker_value(lines[4], "source_snapshot_sha256=")?)?;
    let toolchain = parse_marker_string(lines[5], "toolchain=")?;
    let os = parse_marker_string(lines[6], "os=")?;
    let arch = parse_marker_string(lines[7], "arch=")?;
    let cli = parse_artifact_record(lines[8], "cli=")?;
    let desktop = parse_artifact_record(lines[9], "desktop=")?;

    Ok(ParsedRebuildMarker {
        runner_version,
        source_snapshot_sha256,
        toolchain,
        os,
        arch,
        cli,
        desktop,
    })
}

fn parse_marker_value<'a>(
    line: &'a str,
    prefix: &str,
) -> Result<&'a str, RecoveredSmokeError> {
    line.strip_prefix(prefix)
        .filter(|value| !value.is_empty())
        .ok_or(RecoveredSmokeError::InvalidRebuildMarker)
}

fn parse_marker_string(line: &str, prefix: &str) -> Result<String, RecoveredSmokeError> {
    let value = parse_marker_value(line, prefix)?;
    if value.len() > 128
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control() || byte == b'|')
    {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }
    Ok(value.to_string())
}

fn parse_artifact_record(
    line: &str,
    prefix: &str,
) -> Result<RebuiltArtifact, RecoveredSmokeError> {
    let value = parse_marker_value(line, prefix)?;
    let mut parts = value.split('|');
    let relative_path = parts.next().ok_or(RecoveredSmokeError::InvalidRebuildMarker)?;
    let bytes_text = parts.next().ok_or(RecoveredSmokeError::InvalidRebuildMarker)?;
    let sha_text = parts.next().ok_or(RecoveredSmokeError::InvalidRebuildMarker)?;
    if parts.next().is_some()
        || relative_path.is_empty()
        || relative_path.len() > 512
        || relative_path.contains('\\')
        || relative_path.starts_with('/')
        || relative_path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }

    let bytes = bytes_text
        .parse::<u64>()
        .map_err(|_| RecoveredSmokeError::InvalidRebuildMarker)?;
    if bytes == 0 || bytes > MAX_REBUILD_BINARY_BYTES {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }
    let sha256 = parse_lower_hex_32(sha_text)?;

    Ok(RebuiltArtifact {
        relative_path: relative_path.to_string(),
        bytes,
        sha256,
    })
}

fn parse_lower_hex_32(value: &str) -> Result<[u8; 32], RecoveredSmokeError> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(RecoveredSmokeError::InvalidRebuildMarker);
    }

    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0]).ok_or(RecoveredSmokeError::InvalidRebuildMarker)?;
        let low = hex_nibble(pair[1]).ok_or(RecoveredSmokeError::InvalidRebuildMarker)?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn verify_recorded_artifact(
    rebuild_root: &Path,
    expected: &RebuiltArtifact,
) -> Result<(), RecoveredSmokeError> {
    let path = rebuild_root.join(&expected.relative_path);
    let actual = hash_artifact(&path, &expected.relative_path)
        .map_err(|_| RecoveredSmokeError::ArtifactInvalid)?;
    if actual != *expected {
        return Err(RecoveredSmokeError::ArtifactMismatch);
    }
    Ok(())
}

fn staged_package_version(staged_root: &Path) -> Result<String, RecoveredSmokeError> {
    let path = staged_root.join("Cargo.toml");
    let bytes = read_bounded_regular(
        &path,
        1024 * 1024,
        RecoveredSmokeError::InvalidStagedManifest,
    )?;
    let text =
        String::from_utf8(bytes).map_err(|_| RecoveredSmokeError::InvalidStagedManifest)?;
    let normalized = normalize_newlines(&text);
    let mut in_package = false;

    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = rest.trim();
        if value.len() < 3
            || !value.starts_with('"')
            || !value.ends_with('"')
        {
            return Err(RecoveredSmokeError::InvalidStagedManifest);
        }
        let version = &value[1..value.len() - 1];
        if version.is_empty()
            || version.len() > 64
            || !version.is_ascii()
            || version
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')))
        {
            return Err(RecoveredSmokeError::InvalidStagedManifest);
        }
        return Ok(version.to_string());
    }

    Err(RecoveredSmokeError::InvalidStagedManifest)
}

fn run_smoke_command(
    executable: &Path,
    args: &[&str],
    sandbox: &Path,
    label: &str,
) -> Result<String, RecoveredSmokeError> {
    let stdout_path = sandbox.join(format!("{label}.stdout"));
    let stderr_path = sandbox.join(format!("{label}.stderr"));
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout_path)
        .map_err(|_| RecoveredSmokeError::Io)?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr_path)
        .map_err(|_| RecoveredSmokeError::Io)?;

    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(sandbox)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    sanitize_smoke_environment(&mut command, sandbox);

    let mut child = command
        .spawn()
        .map_err(|_| RecoveredSmokeError::ProcessLaunchFailed)?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err(RecoveredSmokeError::ProcessFailed),
            Ok(None) => {
                if started.elapsed() >= SMOKE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RecoveredSmokeError::ProcessTimedOut);
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RecoveredSmokeError::ProcessFailed);
            }
        }
    }

    let stdout = read_bounded_regular(
        &stdout_path,
        MAX_SMOKE_OUTPUT_BYTES,
        RecoveredSmokeError::UnexpectedOutput,
    )?;
    let stderr = read_bounded_regular(
        &stderr_path,
        MAX_SMOKE_OUTPUT_BYTES,
        RecoveredSmokeError::UnexpectedOutput,
    )?;
    if !stderr.is_empty() {
        return Err(RecoveredSmokeError::UnexpectedOutput);
    }

    String::from_utf8(stdout).map_err(|_| RecoveredSmokeError::UnexpectedOutput)
}

fn sanitize_smoke_environment(command: &mut Command, sandbox: &Path) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("DANGOMUSHI_") {
            command.env_remove(&key);
        }
    }

    command
        .env("HOME", sandbox)
        .env("USERPROFILE", sandbox)
        .env("XDG_CONFIG_HOME", sandbox)
        .env("XDG_CACHE_HOME", sandbox)
        .env("XDG_DATA_HOME", sandbox)
        .env("APPDATA", sandbox)
        .env("LOCALAPPDATA", sandbox)
        .env("TMPDIR", sandbox)
        .env("TMP", sandbox)
        .env("TEMP", sandbox);
}

fn read_bounded_regular(
    path: &Path,
    limit: u64,
    missing_error: RecoveredSmokeError,
) -> Result<Vec<u8>, RecoveredSmokeError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(missing_error),
        Err(_) => return Err(RecoveredSmokeError::Io),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        return Err(missing_error);
    }

    let mut file = File::open(path).map_err(|_| RecoveredSmokeError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len().min(limit) as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RecoveredSmokeError::Io)?;
    if bytes.len() as u64 > limit || bytes.len() as u64 != metadata.len() {
        return Err(missing_error);
    }
    Ok(bytes)
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
