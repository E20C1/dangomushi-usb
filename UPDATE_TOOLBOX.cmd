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

echo [1/5] Running core tests...
cargo test --locked
if errorlevel 1 goto :fail

echo [2/5] Building core CLI...
cargo build --locked --release --bin dangomushi
if errorlevel 1 goto :fail

echo [3/5] Building native desktop shell...
cargo build --locked --release --features desktop --bin dangomushi-desktop
if errorlevel 1 goto :fail

echo [4/5] Staging application...
if exist "build\staging" rmdir /s /q "build\staging"
mkdir "build\staging" >nul 2>nul
mkdir "app" >nul 2>nul
mkdir "build\backup" >nul 2>nul

copy /y "target\release\dangomushi-desktop.exe" "build\staging\Dangomushi.exe" >nul
if errorlevel 1 goto :fail
copy /y "target\release\dangomushi.exe" "build\staging\dangomushi-cli.exe" >nul
if errorlevel 1 goto :fail

echo [5/5] Installing...
if exist "app\Dangomushi.exe" (
  copy /y "app\Dangomushi.exe" "build\backup\Dangomushi.previous.exe" >nul
  if errorlevel 1 goto :fail
)
if exist "app\dangomushi-cli.exe" (
  copy /y "app\dangomushi-cli.exe" "build\backup\dangomushi-cli.previous.exe" >nul
  if errorlevel 1 goto :fail
)

move /y "build\staging\Dangomushi.exe" "app\Dangomushi.exe" >nul
if errorlevel 1 (
  echo [ERROR] Could not replace app\Dangomushi.exe.
  echo Close DANGOMUSHI if it is currently running, then retry.
  goto :fail
)

move /y "build\staging\dangomushi-cli.exe" "app\dangomushi-cli.exe" >nul
if errorlevel 1 goto :fail

echo.
echo [OK] Update complete.
echo      app\Dangomushi.exe
echo      app\dangomushi-cli.exe
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
