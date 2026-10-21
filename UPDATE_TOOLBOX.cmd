@echo off
setlocal
cd /d "%~dp0"

echo.
echo DANGOMUSHI UPDATE
echo =================
echo.

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] cargo was not found in PATH.
  echo Install a Rust toolchain before building from source.
  goto :fail
)

echo [1/4] Running tests...
cargo test
if errorlevel 1 goto :fail

echo [2/4] Building release...
cargo build --release
if errorlevel 1 goto :fail

echo [3/4] Staging application...
if exist "build\staging" rmdir /s /q "build\staging"
mkdir "build\staging" >nul 2>nul
mkdir "app" >nul 2>nul
mkdir "build\backup" >nul 2>nul

copy /y "target\release\dangomushi.exe" "build\staging\Dangomushi.exe" >nul
if errorlevel 1 goto :fail

echo [4/4] Installing...
if exist "app\Dangomushi.exe" (
  copy /y "app\Dangomushi.exe" "build\backup\Dangomushi.previous.exe" >nul
  if errorlevel 1 goto :fail
)

move /y "build\staging\Dangomushi.exe" "app\Dangomushi.exe" >nul
if errorlevel 1 (
  echo [ERROR] Could not replace app\Dangomushi.exe.
  echo Close DANGOMUSHI if it is currently running, then retry.
  goto :fail
)

echo.
echo [OK] Update complete.
echo      app\Dangomushi.exe
echo      workspace\ was not modified.
echo.
pause
exit /b 0

:fail
echo.
echo [FAILED] Existing application and workspace were left in place.
echo.
pause
exit /b 1
