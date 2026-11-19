#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

echo "DANGOMUSHI UPDATE"
echo "================="

cargo test --locked
cargo build --locked --release --bin dangomushi

rm -rf build/staging
mkdir -p build/staging build/backup app

cp target/release/dangomushi build/staging/Dangomushi

if [ -f app/Dangomushi ]; then
  cp app/Dangomushi build/backup/Dangomushi.previous
fi

mv build/staging/Dangomushi app/Dangomushi
chmod +x app/Dangomushi

echo "[OK] app/Dangomushi updated"
echo "[OK] workspace/ was not modified"
