#!/usr/bin/env python3
import argparse
import hashlib
import json
import re
import stat
import tarfile
import zipfile
from pathlib import Path, PurePosixPath

KIND = "dangomushi-portable-release"
FORMAT = 2
MAX_ARCHIVE_FILES = 2048
MAX_MANIFEST_BYTES = 4 * 1024 * 1024
MAX_PAYLOAD_BYTES = 256 * 1024 * 1024
WINDOWS_TERMINAL_KIND = "dangomushi-windows-terminal-runtime"
WINDOWS_TERMINAL_FORMAT = 1
WINDOWS_TERMINAL_VERSION = "1.25.3131.0"
WINDOWS_TERMINAL_SHA256 = {
    "x86_64": "273d6f1502eca1b5bd95c43f5d4d7ec72cc503df340102f8659717e5bead3437",
    "aarch64": "b7e1d2a414e6368affd098d91a4cba441dcacd3ab634a6f39c8e6f5490deef89",
}

def fail(message: str) -> None:
    raise SystemExit(f"[ERROR] {message}")

def valid_archive_path(name: str) -> bool:
    if not name or "\\" in name or name.startswith("/"):
        return False
    path = PurePosixPath(name)
    return not path.is_absolute() and all(part not in {"", ".", ".."} for part in path.parts)

def sha256_stream(handle) -> tuple[int, str]:
    digest = hashlib.sha256()
    total = 0
    while True:
        chunk = handle.read(64 * 1024)
        if not chunk:
            break
        total += len(chunk)
        if total > MAX_PAYLOAD_BYTES:
            fail("archive payload exceeds size bound")
        digest.update(chunk)
    return total, digest.hexdigest()

class ArchiveView:
    def __init__(self, archive: Path):
        self.archive = archive
        self.zip = None
        self.tar = None
        self.entries = {}

        if archive.name.endswith(".zip"):
            self.zip = zipfile.ZipFile(archive, "r")
            for info in self.zip.infolist():
                if info.is_dir():
                    fail("directory entries are not permitted in release archives")
                if not valid_archive_path(info.filename):
                    fail(f"unsafe archive path: {info.filename}")
                if info.filename in self.entries:
                    fail("duplicate archive path")
                mode = (info.external_attr >> 16) & 0xFFFF
                if stat.S_IFMT(mode) not in {0, stat.S_IFREG}:
                    fail("non-regular zip entry")
                self.entries[info.filename] = ("zip", info, mode)
        elif archive.name.endswith(".tar.gz"):
            self.tar = tarfile.open(archive, "r:gz")
            for member in self.tar.getmembers():
                if not member.isfile():
                    fail("only regular files are permitted in release archives")
                if not valid_archive_path(member.name):
                    fail(f"unsafe archive path: {member.name}")
                if member.name in self.entries:
                    fail("duplicate archive path")
                self.entries[member.name] = ("tar", member, member.mode)
        else:
            fail("release archive must end in .zip or .tar.gz")

        if not self.entries or len(self.entries) > MAX_ARCHIVE_FILES:
            fail("unexpected release archive file count")

    def close(self) -> None:
        if self.zip is not None:
            self.zip.close()
        if self.tar is not None:
            self.tar.close()

    def open(self, name: str):
        kind, entry, _ = self.entries[name]
        if kind == "zip":
            return self.zip.open(entry, "r")
        handle = self.tar.extractfile(entry)
        if handle is None:
            fail(f"could not read archive member: {name}")
        return handle

    def mode(self, name: str) -> int:
        return self.entries[name][2]

def expected_payload(os_name: str) -> dict[str, bool]:
    if os_name == "windows":
        return {
            "app/Dangomushi.exe": True,
            "START_DANGOMUSHI.cmd": False,
            "START_HERE.txt": False,
            "runtime/windows-terminal/RUNTIME-MANIFEST.json": False,
        }
    if os_name in {"linux", "macos"}:
        return {
            "app/Dangomushi": True,
            "START_HERE.txt": False,
        }
    fail(f"unsupported manifest OS: {os_name}")

def verify_windows_terminal_runtime(
    view: ArchiveView,
    root: str,
    record_map: dict[str, dict],
    arch: str,
) -> None:
    prefix = "runtime/windows-terminal/"
    manifest_relative = f"{prefix}RUNTIME-MANIFEST.json"
    manifest_name = f"{root}/{manifest_relative}"
    with view.open(manifest_name) as handle:
        data = handle.read(MAX_MANIFEST_BYTES + 1)
    if len(data) > MAX_MANIFEST_BYTES:
        fail("Windows Terminal runtime manifest exceeds size bound")
    try:
        manifest = json.loads(data.decode("utf-8"))
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
        or manifest["asset_sha256"] != WINDOWS_TERMINAL_SHA256[arch]
    ):
        fail("Windows Terminal runtime identity does not match the pinned baseline")

    records = manifest["files"]
    if not isinstance(records, list) or not records or len(records) > MAX_ARCHIVE_FILES:
        fail("Windows Terminal runtime file count is invalid")
    runtime_map = {}
    for record in records:
        if not isinstance(record, dict) or set(record) != {"path", "bytes", "sha256"}:
            fail("Windows Terminal runtime file record is invalid")
        path = record["path"]
        if not isinstance(path, str) or not valid_archive_path(path):
            fail("Windows Terminal runtime contains an unsafe path")
        if path == "RUNTIME-MANIFEST.json" or path in runtime_map:
            fail("Windows Terminal runtime contains a duplicate or reserved path")
        if not isinstance(record["bytes"], int) or record["bytes"] < 0 or record["bytes"] > MAX_PAYLOAD_BYTES:
            fail("Windows Terminal runtime file length is invalid")
        if not isinstance(record["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", record["sha256"]):
            fail("Windows Terminal runtime file digest is invalid")
        runtime_map[path] = record

    actual_runtime = {
        path[len(prefix):]
        for path in record_map
        if path.startswith(prefix) and path != manifest_relative
    }
    if actual_runtime != set(runtime_map):
        fail("Windows Terminal release payload does not match its runtime manifest")

    for required in [".portable", "WindowsTerminal.exe", "wt.exe", "NOTICE.html"]:
        if required not in runtime_map:
            fail(f"Windows Terminal runtime is missing {required}")

    for relative, runtime_record in runtime_map.items():
        release_record = record_map[f"{prefix}{relative}"]
        if (
            release_record["bytes"] != runtime_record["bytes"]
            or release_record["sha256"] != runtime_record["sha256"]
            or release_record["executable"] != relative.lower().endswith(".exe")
        ):
            fail(f"Windows Terminal runtime record mismatch: {relative}")


def check_sidecar(archive: Path, checksum: Path) -> None:
    if checksum.is_symlink() or not checksum.is_file():
        fail("checksum sidecar must be a real regular file")
    text = checksum.read_text(encoding="ascii")
    expected_pattern = rf"^([0-9a-f]{{64}})  {re.escape(archive.name)}\n$"
    match = re.match(expected_pattern, text)
    if not match:
        fail("invalid checksum sidecar format")
    actual = hashlib.sha256(archive.read_bytes()).hexdigest()
    if actual != match.group(1):
        fail("release archive checksum mismatch")

def verify(archive: Path, checksum: Path) -> None:
    if archive.is_symlink() or not archive.is_file():
        fail("release archive must be a real regular file")
    check_sidecar(archive, checksum)

    view = ArchiveView(archive)
    try:
        roots = {PurePosixPath(name).parts[0] for name in view.entries}
        if len(roots) != 1:
            fail("release archive must have exactly one root")
        root = next(iter(roots))
        manifest_name = f"{root}/RELEASE-MANIFEST.json"
        if manifest_name not in view.entries:
            fail("release manifest is missing")

        with view.open(manifest_name) as handle:
            manifest_bytes = handle.read(MAX_MANIFEST_BYTES + 1)
        if len(manifest_bytes) > MAX_MANIFEST_BYTES:
            fail("release manifest exceeds size bound")
        try:
            manifest = json.loads(manifest_bytes.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("release manifest is invalid")

        expected_keys = {
            "kind", "format", "app_version", "os", "arch",
            "bundle_id", "workspace_layout", "files"
        }
        if set(manifest) != expected_keys:
            fail("release manifest fields are not canonical")
        if manifest["kind"] != KIND or manifest["format"] != FORMAT:
            fail("unsupported release manifest")
        if manifest["bundle_id"] != root:
            fail("bundle root does not match manifest")
        if manifest["workspace_layout"] != "workspace-sibling-of-app":
            fail("unexpected workspace layout")
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", manifest["app_version"]):
            fail("invalid application version")
        if manifest["arch"] not in {"x86_64", "aarch64"}:
            fail("unsupported release architecture")

        suffix = ".zip" if archive.name.endswith(".zip") else ".tar.gz"
        if archive.name != f"{root}{suffix}":
            fail("archive filename does not match bundle identity")

        required = expected_payload(manifest["os"])
        records = manifest["files"]
        if not isinstance(records, list) or not records:
            fail("release manifest has no payload records")

        paths = []
        record_map = {}
        for record in records:
            if not isinstance(record, dict) or set(record) != {"path", "bytes", "sha256", "executable"}:
                fail("invalid release file record")
            path = record["path"]
            if not isinstance(path, str) or not valid_archive_path(path):
                fail("invalid release payload path")
            if path in record_map:
                fail("duplicate release payload path")
            if (
                path == "RELEASE-MANIFEST.json"
                or path.startswith("workspace/")
                or path.startswith("vendor/")
                or path.startswith("recovery/")
                or path.startswith(".git/")
                or path.startswith("target/")
                or path.startswith("build/")
            ):
                fail(f"forbidden release payload path: {path}")
            if not isinstance(record["bytes"], int) or record["bytes"] < 0 or record["bytes"] > MAX_PAYLOAD_BYTES:
                fail("invalid release payload length")
            if not isinstance(record["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", record["sha256"]):
                fail("invalid release payload digest")
            if not isinstance(record["executable"], bool):
                fail("invalid executable flag")
            paths.append(path)
            record_map[path] = record

        if paths != sorted(paths):
            fail("release manifest records must be sorted")
        if manifest["os"] == "windows":
            if not set(required).issubset(record_map):
                fail("Windows release payload is missing required files")
            allowed_non_runtime = set(required) - {"runtime/windows-terminal/RUNTIME-MANIFEST.json"}
            unexpected = {
                path for path in record_map
                if path not in allowed_non_runtime
                and not path.startswith("runtime/windows-terminal/")
            }
            if unexpected:
                fail("Windows release payload contains unexpected non-runtime files")
            verify_windows_terminal_runtime(view, root, record_map, manifest["arch"])
        elif set(record_map) != set(required):
            fail("release payload does not match the reviewed OS layout")

        archive_payload = {
            name[len(root) + 1:]
            for name in view.entries
            if name != manifest_name
        }
        if archive_payload != set(record_map):
            fail("archive contains missing or unexpected payload files")

        for path, expected_executable in required.items():
            if record_map[path]["executable"] != expected_executable:
                fail("release executable metadata mismatch")

        for path, record in record_map.items():
            archive_name = f"{root}/{path}"
            with view.open(archive_name) as handle:
                size, digest = sha256_stream(handle)
            if size != record["bytes"] or digest != record["sha256"]:
                fail(f"release payload integrity mismatch: {path}")
            if record["executable"] and (view.mode(archive_name) & 0o111) == 0:
                fail(f"release executable permission missing: {path}")

        print(f"[OK] {archive.name}")
        print(f"[OK] kind={manifest['kind']} format={manifest['format']}")
        print(f"[OK] version={manifest['app_version']} os={manifest['os']} arch={manifest['arch']}")
        print(f"[OK] files={len(records)}")
    finally:
        view.close()

def main() -> None:
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--archive")
    group.add_argument("--directory")
    parser.add_argument("--checksum")
    args = parser.parse_args()

    if args.directory:
        directory = Path(args.directory)
        archives = sorted(
            [p for p in directory.glob("*.zip") if p.is_file()]
            + [p for p in directory.glob("*.tar.gz") if p.is_file()]
        )
        if len(archives) != 1:
            fail("release directory must contain exactly one archive")
        archive = archives[0]
        checksum = Path(str(archive) + ".sha256")
    else:
        archive = Path(args.archive)
        checksum = Path(args.checksum) if args.checksum else Path(str(archive) + ".sha256")

    verify(archive, checksum)

if __name__ == "__main__":
    main()
