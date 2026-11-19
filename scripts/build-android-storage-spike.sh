#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

echo "DANGOMUSHI ANDROID STORAGE CONTRACT SPIKE"
echo "=========================================="

command -v cargo >/dev/null 2>&1 || {
  echo "[ERROR] cargo was not found in PATH." >&2
  exit 1
}

command -v cargo-apk >/dev/null 2>&1 || {
  echo "[ERROR] cargo-apk 0.10.0 was not found in PATH." >&2
  exit 1
}

if [ -z "${ANDROID_HOME:-}" ] && [ -z "${ANDROID_SDK_ROOT:-}" ]; then
  echo "[ERROR] ANDROID_HOME or ANDROID_SDK_ROOT is required." >&2
  exit 1
fi

if [ -z "${ANDROID_NDK_HOME:-}" ] && [ -z "${ANDROID_NDK_ROOT:-}" ]; then
  echo "[ERROR] ANDROID_NDK_HOME or ANDROID_NDK_ROOT is required." >&2
  exit 1
fi

rustup target list --installed | grep -qx "aarch64-linux-android" || {
  echo "[ERROR] Rust target aarch64-linux-android is not installed." >&2
  exit 1
}

(
  cd android-spike
  cargo test --locked
  cargo apk build --release --locked
)

echo "[OK] Android storage-contract spike built."
echo "     Inspect android-spike/target/release/apk for the APK."
