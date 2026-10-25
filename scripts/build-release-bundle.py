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
FORMAT = 1
MAX_BINARY_BYTES = 256 * 1024 * 1024

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

def start_here(version: str, os_name: str) -> str:
    if os_name == "windows":
        launch = "Double-click app\\Dangomushi.exe\nCLI: app\\dangomushi-cli.exe"
    elif os_name == "macos":
        launch = "Open app/Dangomushi.app\nCLI: app/dangomushi-cli"
    else:
        launch = "Run app/Dangomushi\nCLI: app/dangomushi-cli"

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
        "Documentation is embedded in the binaries. Use the SYSTEM view or:\n"
        "  dangomushi-cli docs list\n\n"
        "SECURITY / DISTRIBUTION\n"
        "-----------------------\n"
        "Verify the archive against its .sha256 sidecar before extraction.\n"
        "This bundle is not code-signed or notarized by this task; operating-system trust\n"
        "prompts may therefore appear. SHA-256 proves file equality, not publisher identity.\n"
    )

def macos_plist(version: str) -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>DANGOMUSHI</string>
  <key>CFBundleExecutable</key><string>Dangomushi</string>
  <key>CFBundleIdentifier</key><string>io.github.e20c1.dangomushi</string>
  <key>CFBundleName</key><string>DANGOMUSHI</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"""

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
        cli_source = input_dir / "dangomushi.exe"
        desktop_source = input_dir / "dangomushi-desktop.exe"
        cli_relative = "app/dangomushi-cli.exe"
        desktop_relative = "app/Dangomushi.exe"
    else:
        archive_name = f"{bundle_id}.tar.gz"
        cli_source = input_dir / "dangomushi"
        desktop_source = input_dir / "dangomushi-desktop"
        cli_relative = "app/dangomushi-cli"
        desktop_relative = (
            "app/Dangomushi.app/Contents/MacOS/Dangomushi"
            if os_name == "macos"
            else "app/Dangomushi"
        )

    archive_path = output_dir / archive_name
    checksum_path = output_dir / f"{archive_name}.sha256"
    if archive_path.exists() or checksum_path.exists():
        fail("release output already exists; refusing silent overwrite")

    with tempfile.TemporaryDirectory(prefix=".dangomushi-release-", dir=output_dir) as temporary:
        staging = Path(temporary) / bundle_id
        staging.mkdir()

        executable_paths = {desktop_relative, cli_relative}
        copy_payload(desktop_source, staging / desktop_relative, True)
        copy_payload(cli_source, staging / cli_relative, True)

        (staging / "START_HERE.txt").write_text(
            start_here(version, os_name), encoding="utf-8", newline="\n"
        )
        (staging / "START_HERE.txt").chmod(0o644)

        if os_name == "macos":
            plist = staging / "app/Dangomushi.app/Contents/Info.plist"
            plist.parent.mkdir(parents=True, exist_ok=True)
            plist.write_text(macos_plist(version), encoding="utf-8", newline="\n")
            plist.chmod(0o644)

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
