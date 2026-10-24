#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

if [ ! -f Cargo.lock ] || [ ! -d vendor ]; then
  echo "[ERROR] Cargo.lock and vendor/ are required." >&2
  echo "Run scripts/vendor-dependencies.sh while dependencies are still available." >&2
  exit 1
fi

CARGO_VENDOR_CONFIG=".cargo/vendor-config.toml"

echo "DANGOMUSHI FROZEN VENDOR VERIFY"
echo "==============================="
echo "[1/5] Frozen metadata..."
cargo --config "$CARGO_VENDOR_CONFIG" metadata --frozen --format-version 1 >/dev/null

echo "[2/5] Core tests..."
cargo --config "$CARGO_VENDOR_CONFIG" test --frozen

echo "[3/5] Core release build..."
cargo --config "$CARGO_VENDOR_CONFIG" build --frozen --release --bin dangomushi

echo "[4/5] Desktop library tests..."
cargo --config "$CARGO_VENDOR_CONFIG" test --frozen --features desktop --lib

echo "[5/5] Native desktop release build..."
cargo --config "$CARGO_VENDOR_CONFIG" build --frozen --release --features desktop --bin dangomushi-desktop

echo "[OK] Vendored Cargo sources completed the frozen build/test path without registry access."
