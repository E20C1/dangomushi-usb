#!/usr/bin/env python3
from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
match = re.search(r'(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"\s*$', CARGO_TOML)
if not match:
    raise SystemExit("could not determine package version from Cargo.toml")
version = match.group(1)

suffix = ".exe" if os.name == "nt" else ""
release = ROOT / "target" / "release"
cli = release / f"dangomushi{suffix}"
desktop = release / f"dangomushi-desktop{suffix}"

for path in (cli, desktop):
    if not path.is_file():
        raise SystemExit(f"missing rebuilt smoke entrypoint: {path}")

def sanitized_environment(sandbox: Path) -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if not key.startswith("DANGOMUSHI_")}
    sandbox_text = str(sandbox)
    for key in [
        "HOME",
        "USERPROFILE",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "TMPDIR",
        "TMP",
        "TEMP",
    ]:
        env[key] = sandbox_text
    return env

def run(path: Path, args: list[str], expected: str, sandbox: Path) -> None:
    result = subprocess.run(
        [str(path), *args],
        cwd=sandbox,
        env=sanitized_environment(sandbox),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=15,
        check=False,
    )
    stdout = result.stdout.decode("utf-8").replace("\r\n", "\n").replace("\r", "\n")
    stderr = result.stderr.decode("utf-8").replace("\r\n", "\n").replace("\r", "\n")
    if result.returncode != 0:
        raise SystemExit(f"{path.name} smoke entrypoint failed with {result.returncode}: {stderr}")
    if stderr:
        raise SystemExit(f"{path.name} smoke entrypoint wrote stderr: {stderr!r}")
    if stdout != expected:
        raise SystemExit(
            f"{path.name} smoke output mismatch: expected {expected!r}, got {stdout!r}"
        )

with tempfile.TemporaryDirectory(prefix="dangomushi-ci-smoke-") as tmp:
    sandbox = Path(tmp)
    run(cli, ["version"], f"dangomushi {version}\n", sandbox)
    run(
        desktop,
        ["--recovery-smoke-check"],
        f"dangomushi-desktop {version} recovery-smoke-ok\n",
        sandbox,
    )

print("recovered application smoke entrypoints: OK")
