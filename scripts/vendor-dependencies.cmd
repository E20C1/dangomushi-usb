@echo off
setlocal
cd /d "%~dp0\.."

if not exist "Cargo.lock" (
  echo [ERROR] Cargo.lock is required. Refusing an unlocked vendor snapshot.
  exit /b 1
)

if exist "vendor" (
  echo [ERROR] vendor already exists.
  echo Remove it explicitly before creating a new dependency snapshot.
  exit /b 1
)
if exist "vendor.pending" (
  echo [ERROR] vendor.pending already exists.
  echo Remove it explicitly before creating a new dependency snapshot.
  exit /b 1
)

if not exist "build" mkdir "build" >nul 2>nul

echo DANGOMUSHI DEPENDENCY VENDOR
echo ============================
echo [1/3] Vendoring Cargo.lock resolution...
cargo vendor --locked --versioned-dirs vendor.pending > "build\vendor-source-config.txt"
if errorlevel 1 goto :fail_pending

move "vendor.pending" "vendor" >nul
if errorlevel 1 goto :fail_pending

echo [2/3] Verifying source replacement with network disabled...
cargo --config ".cargo/vendor-config.toml" metadata --frozen --format-version 1 >nul
if errorlevel 1 goto :fail_vendor

echo [3/3] Snapshot ready.
echo [OK] vendorecho [OK] Cargo.lock remains authoritative.
echo [OK] Run scripts\verify-vendored-dependencies.cmd for a full offline build/test verification.
exit /b 0

:fail_pending
if exist "vendor.pending" rmdir /s /q "vendor.pending"
echo [FAILED] cargo vendor failed.
exit /b 1

:fail_vendor
if exist "vendor" rmdir /s /q "vendor"
echo [FAILED] vendored dependency metadata did not resolve offline.
exit /b 1
