#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

echo "DANGOMUSHI UPDATE"
echo "================="

cargo test
cargo build --release --bin dangomushi
cargo build --release --features desktop --bin dangomushi-desktop

rm -rf build/staging
mkdir -p build/staging build/backup app

cp target/release/dangomushi-desktop build/staging/Dangomushi
cp target/release/dangomushi build/staging/dangomushi-cli

if [ -f app/Dangomushi ]; then
  cp app/Dangomushi build/backup/Dangomushi.previous
fi

if [ -f app/dangomushi-cli ]; then
  cp app/dangomushi-cli build/backup/dangomushi-cli.previous
fi

mv build/staging/Dangomushi app/Dangomushi
mv build/staging/dangomushi-cli app/dangomushi-cli
chmod +x app/Dangomushi app/dangomushi-cli

echo "[OK] app/Dangomushi updated"
echo "[OK] app/dangomushi-cli updated"
echo "[OK] workspace/ was not modified"
