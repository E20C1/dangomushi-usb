@echo off
setlocal
cd /d "%~dp0\.."

if not exist "Cargo.lock" (
  echo [ERROR] Cargo.lock is required.
  exit /b 1
)
if not exist "vendor" (
  echo [ERROR] vendor is required.
  echo Run scripts\vendor-dependencies.cmd while dependencies are still available.
  exit /b 1
)

set "VENDOR_CONFIG=.cargo/vendor-config.toml"

echo DANGOMUSHI FROZEN VENDOR VERIFY
echo ===============================
echo [1/3] Frozen metadata...
cargo --config "%VENDOR_CONFIG%" metadata --frozen --format-version 1 >nul
if errorlevel 1 goto :fail

echo [2/3] Core tests...
cargo --config "%VENDOR_CONFIG%" test --frozen
if errorlevel 1 goto :fail

echo [3/3] Application release build...
cargo --config "%VENDOR_CONFIG%" build --frozen --release --bin dangomushi
if errorlevel 1 goto :fail

echo [OK] Vendored Cargo sources completed the frozen build/test path without registry access.
exit /b 0

:fail
echo [FAILED] Frozen vendored build verification failed.
exit /b 1
