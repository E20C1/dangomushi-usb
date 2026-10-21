#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

echo "DANGOMUSHI UPDATE"
echo "================="

cargo test
cargo build --release

rm -rf build/staging
mkdir -p build/staging build/backup app

cp target/release/dangomushi build/staging/dangomushi

if [ -f app/dangomushi ]; then
  cp app/dangomushi build/backup/dangomushi.previous
fi

mv build/staging/dangomushi app/dangomushi
chmod +x app/dangomushi

echo "[OK] app/dangomushi updated"
echo "[OK] workspace/ was not modified"
