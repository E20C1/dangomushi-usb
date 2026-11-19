#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import shutil
import stat
import tempfile
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

KIND = "dangomushi-windows-terminal-runtime"
FORMAT = 1
VERSION = "1.25.3131.0"
MAX_FILES = 2048
MAX_TOTAL_BYTES = 256 * 1024 * 1024
MAX_FILE_BYTES = 64 * 1024 * 1024
ASSETS = {
    "x86_64": {
        "name": "Microsoft.WindowsTerminal_1.25.3131.0_x64.zip",
        "sha256": "273d6f1502eca1b5bd95c43f5d4d7ec72cc503df340102f8659717e5bead3437",
        "bytes": 12126938,
    },
    "aarch64": {
        "name": "Microsoft.WindowsTerminal_1.25.3131.0_arm64.zip",
        "sha256": "b7e1d2a414e6368affd098d91a4cba441dcacd3ab634a6f39c8e6f5490deef89",
        "bytes": 12243850,
    },
}
RELEASE_BASE = "https://github.com/microsoft/terminal/releases/download/v1.25.3131.0"


def fail(message: str) -> None:
    raise SystemExit(f"[ERROR] {message}")


def sha256_file(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(64 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_FILE_BYTES:
                fail(f"runtime file exceeds bound: {path}")
            digest.update(chunk)
    return total, digest.hexdigest()


def download(asset: dict[str, object], destination: Path) -> None:
    url = f"{RELEASE_BASE}/{asset['name']}"
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "dangomushi-runtime-fetch/1"},
    )
    digest = hashlib.sha256()
    total = 0
    with urllib.request.urlopen(request, timeout=120) as response, destination.open("xb") as out:
        while True:
            chunk = response.read(64 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > int(asset["bytes"]):
                fail("Windows Terminal asset exceeded pinned byte length")
            digest.update(chunk)
            out.write(chunk)
    if total != int(asset["bytes"]):
        fail(f"Windows Terminal asset length mismatch: expected {asset['bytes']}, got {total}")
    if digest.hexdigest() != asset["sha256"]:
        fail("Windows Terminal asset SHA-256 mismatch")


def safe_member_path(name: str) -> PurePosixPath:
    path = PurePosixPath(name)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        fail(f"unsafe Windows Terminal archive path: {name}")
    return path


def extract_runtime(archive: Path, destination: Path) -> None:
    with zipfile.ZipFile(archive, "r") as bundle:
        infos = bundle.infolist()
        if not infos or len(infos) > MAX_FILES:
            fail("unexpected Windows Terminal archive file count")

        roots = {safe_member_path(info.filename).parts[0] for info in infos}
        if len(roots) != 1:
            fail("Windows Terminal archive must contain one root directory")
        root = next(iter(roots))
        expected_root = f"terminal-{VERSION}"
        if root != expected_root:
            fail(f"unexpected Windows Terminal archive root: {root}")

        seen: set[str] = set()
        total = 0
        for info in infos:
            path = safe_member_path(info.filename)
            relative_parts = path.parts[1:]
            if not relative_parts:
                continue
            if info.is_dir():
                continue

            mode = (info.external_attr >> 16) & 0xFFFF
            file_type = stat.S_IFMT(mode)
            if file_type not in {0, stat.S_IFREG}:
                fail(f"non-regular Windows Terminal archive member: {info.filename}")

            relative = PurePosixPath(*relative_parts)
            key = relative.as_posix()
            if key in seen:
                fail(f"duplicate Windows Terminal archive member: {key}")
            seen.add(key)

            if info.file_size < 0 or info.file_size > MAX_FILE_BYTES:
                fail(f"Windows Terminal archive member exceeds bound: {key}")
            total += info.file_size
            if total > MAX_TOTAL_BYTES:
                fail("Windows Terminal runtime exceeds total byte bound")

            target = destination.joinpath(*relative.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            with bundle.open(info, "r") as source, target.open("xb") as out:
                shutil.copyfileobj(source, out, length=64 * 1024)
            if target.stat().st_size != info.file_size:
                fail(f"Windows Terminal extracted size mismatch: {key}")

    for required in ["WindowsTerminal.exe", "wt.exe", "NOTICE.html"]:
        path = destination / required
        if path.is_symlink() or not path.is_file():
            fail(f"Windows Terminal runtime is missing {required}")

    (destination / ".portable").write_bytes(b"")


def write_manifest(destination: Path, arch: str, asset: dict[str, object]) -> None:
    records = []
    total = 0
    for path in sorted(destination.rglob("*")):
        if path.is_dir():
            continue
        if path.is_symlink() or not path.is_file():
            fail(f"unsafe Windows Terminal runtime output: {path}")
        relative = path.relative_to(destination).as_posix()
        if relative == "RUNTIME-MANIFEST.json":
            continue
        size, digest = sha256_file(path)
        total += size
        if total > MAX_TOTAL_BYTES:
            fail("Windows Terminal runtime exceeds total byte bound")
        records.append({"path": relative, "bytes": size, "sha256": digest})

    manifest = {
        "kind": KIND,
        "format": FORMAT,
        "version": VERSION,
        "architecture": arch,
        "asset_name": asset["name"],
        "asset_bytes": asset["bytes"],
        "asset_sha256": asset["sha256"],
        "files": records,
    }
    (destination / "RUNTIME-MANIFEST.json").write_text(
        json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--arch", choices=sorted(ASSETS), default="x86_64")
    parser.add_argument("--output-root", default="runtime-cache/windows-terminal")
    args = parser.parse_args()

    asset = ASSETS[args.arch]
    root = Path(args.output_root).resolve()
    final = root / VERSION / args.arch
    if final.exists():
        fail(f"runtime destination already exists: {final}")

    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".windows-terminal-fetch-", dir=root) as tmp:
        tmp_path = Path(tmp)
        archive = tmp_path / str(asset["name"])
        pending = tmp_path / "runtime"
        pending.mkdir()

        download(asset, archive)
        extract_runtime(archive, pending)
        write_manifest(pending, args.arch, asset)

        final.parent.mkdir(parents=True, exist_ok=True)
        os.replace(pending, final)

    print(f"[OK] Windows Terminal {VERSION} / {args.arch}")
    print(f"[OK] runtime: {final}")
    print(f"[OK] asset sha256: {asset['sha256']}")


if __name__ == "__main__":
    main()
