#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

if [ ! -f Cargo.lock ]; then
  echo "[ERROR] Cargo.lock is required. Refusing an unlocked vendor snapshot." >&2
  exit 1
fi

if [ -e vendor ] || [ -e vendor.pending ]; then
  echo "[ERROR] vendor/ or vendor.pending/ already exists." >&2
  echo "Remove it explicitly before creating a new dependency snapshot." >&2
  exit 1
fi

mkdir -p build
echo "DANGOMUSHI DEPENDENCY VENDOR"
echo "============================"
echo "[1/3] Vendoring Cargo.lock resolution..."

if ! cargo vendor --locked --versioned-dirs vendor.pending > build/vendor-source-config.txt; then
  rm -rf vendor.pending
  echo "[FAILED] cargo vendor failed." >&2
  exit 1
fi

mv vendor.pending vendor

echo "[2/3] Verifying source replacement with network disabled..."
if ! cargo --config .cargo/vendor-config.toml metadata --frozen --format-version 1 >/dev/null; then
  rm -rf vendor
  echo "[FAILED] vendored dependency metadata did not resolve offline." >&2
  exit 1
fi

echo "[3/3] Snapshot ready."
echo "[OK] vendor/"
echo "[OK] Cargo.lock remains authoritative."
echo "[OK] Run scripts/verify-vendored-dependencies.sh for a full offline build/test verification."
