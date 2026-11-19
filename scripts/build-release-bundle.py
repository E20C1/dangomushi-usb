#!/usr/bin/env python3
import argparse
import gzip
import hashlib
import io
import json
import os
import platform
import re
import shutil
import stat
import tarfile
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

KIND = "dangomushi-portable-release"
FORMAT = 2
MAX_BINARY_BYTES = 256 * 1024 * 1024
MAX_RUNTIME_FILES = 2048
RUNTIME_MANIFEST_MAX_BYTES = 4 * 1024 * 1024
WINDOWS_TERMINAL_KIND = "dangomushi-windows-terminal-runtime"
WINDOWS_TERMINAL_FORMAT = 1
WINDOWS_TERMINAL_VERSION = "1.25.3131.0"
WINDOWS_TERMINAL_ASSETS = {
    "x86_64": {
        "name": "Microsoft.WindowsTerminal_1.25.3131.0_x64.zip",
        "bytes": 12126938,
        "sha256": "273d6f1502eca1b5bd95c43f5d4d7ec72cc503df340102f8659717e5bead3437",
    },
    "aarch64": {
        "name": "Microsoft.WindowsTerminal_1.25.3131.0_arm64.zip",
        "bytes": 12243850,
        "sha256": "b7e1d2a414e6368affd098d91a4cba441dcacd3ab634a6f39c8e6f5490deef89",
    },
}


def fail(message: str) -> None:
    raise SystemExit(f"[ERROR] {message}")

def normalize_platform() -> tuple[str, str]:
    system = platform.system().lower()
    if system == "windows":
        os_name = "windows"
    elif system == "linux":
        os_name = "linux"
    elif system == "darwin":
        os_name = "macos"
    else:
        fail(f"unsupported operating system: {platform.system()}")

    machine = platform.machine().lower()
    if machine in {"amd64", "x86_64"}:
        arch = "x86_64"
    elif machine in {"arm64", "aarch64"}:
        arch = "aarch64"
    else:
        fail(f"unsupported architecture: {platform.machine()}")

    return os_name, arch

def app_version(root: Path) -> str:
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', text)
    if not match:
        fail("could not read package version from Cargo.toml")
    return match.group(1)

def sha256_file(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(64 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_BINARY_BYTES:
                fail(f"file exceeds release bound: {path}")
            digest.update(chunk)
    return total, digest.hexdigest()

def require_regular(path: Path) -> None:
    if path.is_symlink():
        fail(f"release input must not be a symlink: {path}")
    try:
        mode = path.stat().st_mode
    except FileNotFoundError:
        fail(f"required release input is missing: {path}")
    if not stat.S_ISREG(mode):
        fail(f"release input must be a regular file: {path}")

def copy_payload(source: Path, destination: Path, executable: bool) -> None:
    require_regular(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    if executable:
        destination.chmod(0o755)
    else:
        destination.chmod(0o644)

def valid_relative_path(value: str) -> bool:
    path = PurePosixPath(value)
    return (
        bool(value)
        and not path.is_absolute()
        and all(part not in {"", ".", ".."} for part in path.parts)
    )


def validate_windows_terminal_runtime(runtime: Path, arch: str) -> list[dict]:
    manifest_path = runtime / "RUNTIME-MANIFEST.json"
    require_regular(manifest_path)
    if manifest_path.stat().st_size > RUNTIME_MANIFEST_MAX_BYTES:
        fail("Windows Terminal runtime manifest exceeds size bound")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("Windows Terminal runtime manifest is invalid")

    expected_keys = {
        "kind", "format", "version", "architecture",
        "asset_name", "asset_bytes", "asset_sha256", "files",
    }
    if not isinstance(manifest, dict) or set(manifest) != expected_keys:
        fail("Windows Terminal runtime manifest fields are not canonical")
    if (
        manifest["kind"] != WINDOWS_TERMINAL_KIND
        or manifest["format"] != WINDOWS_TERMINAL_FORMAT
        or manifest["version"] != WINDOWS_TERMINAL_VERSION
        or manifest["architecture"] != arch
        or manifest["asset_name"] != WINDOWS_TERMINAL_ASSETS[arch]["name"]
        or manifest["asset_bytes"] != WINDOWS_TERMINAL_ASSETS[arch]["bytes"]
        or manifest["asset_sha256"] != WINDOWS_TERMINAL_ASSETS[arch]["sha256"]
    ):
        fail("Windows Terminal runtime identity does not match the pinned baseline")

    records = manifest["files"]
    if not isinstance(records, list) or not records or len(records) > MAX_RUNTIME_FILES:
        fail("Windows Terminal runtime manifest has invalid file count")
    record_map = {}
    for record in records:
        if not isinstance(record, dict) or set(record) != {"path", "bytes", "sha256"}:
            fail("Windows Terminal runtime file record is invalid")
        path = record["path"]
        if not isinstance(path, str) or not valid_relative_path(path):
            fail("Windows Terminal runtime contains an unsafe path")
        if path == "RUNTIME-MANIFEST.json" or path in record_map:
            fail("Windows Terminal runtime contains a duplicate or reserved path")
        if not isinstance(record["bytes"], int) or record["bytes"] < 0 or record["bytes"] > MAX_BINARY_BYTES:
            fail("Windows Terminal runtime file length is invalid")
        if not isinstance(record["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", record["sha256"]):
            fail("Windows Terminal runtime file digest is invalid")
        record_map[path] = record

    actual = {}
    for path in runtime.rglob("*"):
        if path.is_dir():
            continue
        if path.is_symlink() or not path.is_file():
            fail(f"Windows Terminal runtime contains unsafe entry: {path}")
        relative = path.relative_to(runtime).as_posix()
        if relative == "RUNTIME-MANIFEST.json":
            continue
        actual[relative] = path

    if set(actual) != set(record_map):
        fail("Windows Terminal runtime file set does not match its manifest")
    for relative, path in actual.items():
        size, digest = sha256_file(path)
        record = record_map[relative]
        if size != record["bytes"] or digest != record["sha256"]:
            fail(f"Windows Terminal runtime integrity mismatch: {relative}")

    for required in [".portable", "WindowsTerminal.exe", "wt.exe", "NOTICE.html"]:
        if required not in record_map:
            fail(f"Windows Terminal runtime is missing {required}")
    return records


def copy_windows_terminal_runtime(source: Path, destination: Path, arch: str) -> set[str]:
    records = validate_windows_terminal_runtime(source, arch)
    executable_paths = set()
    for record in records:
        relative = record["path"]
        src = source.joinpath(*PurePosixPath(relative).parts)
        dst_relative = f"runtime/windows-terminal/{relative}"
        executable = relative.lower().endswith(".exe")
        copy_payload(src, destination / dst_relative, executable)
        if executable:
            executable_paths.add(dst_relative)

    copy_payload(
        source / "RUNTIME-MANIFEST.json",
        destination / "runtime/windows-terminal/RUNTIME-MANIFEST.json",
        False,
    )
    return executable_paths


def windows_launcher() -> str:
    return (
        "@echo off\r\n"
        "setlocal\r\n"
        "cd /d \"%~dp0\"\r\n"
        "set \"DANGOMUSHI_APP=%~dp0app\\Dangomushi.exe\"\r\n"
        "set \"DANGOMUSHI_WT=%~dp0runtime\\windows-terminal\\wt.exe\"\r\n"
        "if exist \"%DANGOMUSHI_WT%\" (\r\n"
        "  \"%DANGOMUSHI_WT%\" -w -1 new-tab --startingDirectory \"%~dp0\" \"%DANGOMUSHI_APP%\"\r\n"
        "  if not errorlevel 1 exit /b 0\r\n"
        ")\r\n"
        "\"%DANGOMUSHI_APP%\"\r\n"
        "exit /b %errorlevel%\r\n"
    )


def start_here(version: str, os_name: str) -> str:
    if os_name == "windows":
        launch = (
            "Double-click START_DANGOMUSHI.cmd for the verified portable terminal + TUI\n"
            "Direct fallback / CLI: app\\Dangomushi.exe"
        )
    else:
        launch = "Run app/Dangomushi for the TUI\nCLI commands use the same app/Dangomushi"

    return (
        f"DANGOMUSHI {version} PORTABLE RELEASE\n"
        f"==================================\n\n"
        f"{launch}\n\n"
        "WORKSPACE\n"
        "---------\n"
        "The release bundle does not contain Workspace data.\n"
        "When the application is launched from this bundle, its portable Workspace is the\n"
        "workspace/ directory beside app/. Initialize it explicitly on first use.\n\n"
        "OFFLINE DOCUMENTATION\n"
        "---------------------\n"
        "Documentation is embedded in the application. Use the SYSTEM view or:\n"
        "  app/Dangomushi docs list\n\n"
        "SECURITY / DISTRIBUTION\n"
        "-----------------------\n"
        "Verify the archive against its .sha256 sidecar before extraction.\n"
        "This bundle is not code-signed or notarized by this task; operating-system trust\n"
        "prompts may therefore appear. SHA-256 proves file equality, not publisher identity.\n"
    )

def collect_records(bundle_root: Path, executable_paths: set[str]) -> list[dict]:
    records = []
    files = [
        (path.relative_to(bundle_root).as_posix(), path)
        for path in bundle_root.rglob("*")
        if path.is_file() and path.name != "RELEASE-MANIFEST.json"
    ]
    for relative, path in sorted(files, key=lambda item: item[0]):
        if path.is_symlink():
            fail(f"bundle staging contains a symlink: {path}")
        pure = PurePosixPath(relative)
        if pure.is_absolute() or ".." in pure.parts or "." in pure.parts:
            fail(f"invalid bundle path: {relative}")
        size, digest = sha256_file(path)
        records.append({
            "path": relative,
            "bytes": size,
            "sha256": digest,
            "executable": relative in executable_paths,
        })
    return records

def write_manifest(bundle_root: Path, version: str, os_name: str, arch: str, bundle_id: str, records: list[dict]) -> None:
    manifest = {
        "kind": KIND,
        "format": FORMAT,
        "app_version": version,
        "os": os_name,
        "arch": arch,
        "bundle_id": bundle_id,
        "workspace_layout": "workspace-sibling-of-app",
        "files": records,
    }
    encoded = json.dumps(manifest, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
    (bundle_root / "RELEASE-MANIFEST.json").write_text(encoded, encoding="utf-8", newline="\n")

def archive_files(bundle_root: Path) -> list[Path]:
    return [
        path
        for _, path in sorted(
            (
                (path.relative_to(bundle_root).as_posix(), path)
                for path in bundle_root.rglob("*")
                if path.is_file()
            ),
            key=lambda item: item[0],
        )
    ]

def write_zip(bundle_root: Path, bundle_id: str, output: Path, executable_paths: set[str]) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in archive_files(bundle_root):
            relative = path.relative_to(bundle_root).as_posix()
            name = f"{bundle_id}/{relative}"
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            mode = 0o755 if relative in executable_paths else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            archive.writestr(info, path.read_bytes())

def write_tar_gz(bundle_root: Path, bundle_id: str, output: Path, executable_paths: set[str]) -> None:
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for path in archive_files(bundle_root):
                    relative = path.relative_to(bundle_root).as_posix()
                    data = path.read_bytes()
                    info = tarfile.TarInfo(f"{bundle_id}/{relative}")
                    info.size = len(data)
                    info.mode = 0o755 if relative in executable_paths else 0o644
                    info.mtime = 0
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    archive.addfile(info, io.BytesIO(data))

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--input-dir", default="target/release")
    parser.add_argument("--output-dir", default="dist")
    parser.add_argument("--windows-terminal-runtime")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    input_dir = (root / args.input_dir).resolve()
    output_dir = (root / args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    version = app_version(root)
    os_name, arch = normalize_platform()
    bundle_id = f"dangomushi-{version}-{os_name}-{arch}"

    if os_name == "windows":
        archive_name = f"{bundle_id}.zip"
        application_source = input_dir / "dangomushi.exe"
        application_relative = "app/Dangomushi.exe"
    else:
        archive_name = f"{bundle_id}.tar.gz"
        application_source = input_dir / "dangomushi"
        application_relative = "app/Dangomushi"

    archive_path = output_dir / archive_name
    checksum_path = output_dir / f"{archive_name}.sha256"
    if archive_path.exists() or checksum_path.exists():
        fail("release output already exists; refusing silent overwrite")

    with tempfile.TemporaryDirectory(prefix=".dangomushi-release-", dir=output_dir) as temporary:
        staging = Path(temporary) / bundle_id
        staging.mkdir()

        executable_paths = {application_relative}
        copy_payload(application_source, staging / application_relative, True)

        if os_name == "windows":
            runtime = (
                Path(args.windows_terminal_runtime).resolve()
                if args.windows_terminal_runtime
                else root / "runtime-cache" / "windows-terminal" / WINDOWS_TERMINAL_VERSION / arch
            )
            executable_paths.update(
                copy_windows_terminal_runtime(runtime, staging, arch)
            )
            (staging / "START_DANGOMUSHI.cmd").write_text(
                windows_launcher(), encoding="utf-8", newline=""
            )
            (staging / "START_DANGOMUSHI.cmd").chmod(0o644)

        (staging / "START_HERE.txt").write_text(
            start_here(version, os_name), encoding="utf-8", newline="\n"
        )
        (staging / "START_HERE.txt").chmod(0o644)

        records = collect_records(staging, executable_paths)
        write_manifest(staging, version, os_name, arch, bundle_id, records)

        temporary_archive = output_dir / f".{archive_name}.pending"
        try:
            if os_name == "windows":
                write_zip(staging, bundle_id, temporary_archive, executable_paths)
            else:
                write_tar_gz(staging, bundle_id, temporary_archive, executable_paths)
            os.replace(temporary_archive, archive_path)
        finally:
            if temporary_archive.exists():
                temporary_archive.unlink()

    archive_digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path.write_text(f"{archive_digest}  {archive_name}\n", encoding="ascii", newline="\n")

    print(f"[OK] {archive_path}")
    print(f"[OK] {checksum_path}")

if __name__ == "__main__":
    main()
