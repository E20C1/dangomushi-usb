@echo off
setlocal
cd /d "%~dp0\.."

echo.
echo DANGOMUSHI ANDROID STORAGE CONTRACT SPIKE
echo ==========================================
echo.

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] cargo was not found in PATH.
  exit /b 1
)

where cargo-apk >nul 2>nul
if errorlevel 1 (
  echo [ERROR] cargo-apk 0.10.0 was not found in PATH.
  echo         Install it explicitly before running this A0 spike.
  exit /b 1
)

if not defined ANDROID_HOME if not defined ANDROID_SDK_ROOT (
  echo [ERROR] ANDROID_HOME or ANDROID_SDK_ROOT is required.
  exit /b 1
)

if not defined ANDROID_NDK_HOME if not defined ANDROID_NDK_ROOT (
  echo [ERROR] ANDROID_NDK_HOME or ANDROID_NDK_ROOT is required.
  exit /b 1
)

rustup target list --installed | findstr /x "aarch64-linux-android" >nul
if errorlevel 1 (
  echo [ERROR] Rust target aarch64-linux-android is not installed.
  exit /b 1
)

pushd android-spike
cargo test
if errorlevel 1 (
  popd
  exit /b 1
)

cargo apk build
if errorlevel 1 (
  popd
  exit /b 1
)
popd

echo.
echo [OK] Android storage-contract spike built.
echo      Inspect android-spike\target\debug\apk for the APK.
exit /b 0
