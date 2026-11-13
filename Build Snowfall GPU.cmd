@echo off
setlocal
chcp 65001 > nul
cd /d "%~dp0windows"
where cargo >nul 2>nul
if errorlevel 1 (
  echo ERROR: Rust is not installed.
  echo Install stable Rust from https://rustup.rs
  echo Also install Visual Studio 2022 Build Tools with Desktop development with C++.
  echo Then run this script again.
  pause
  exit /b 1
)
echo Building SnowfallGPU.exe using locked dependencies...
echo First build downloads Rust libraries (network is required only for building).
cargo build --release --locked --bin SnowfallGPU
if errorlevel 1 (
  echo.
  echo BUILD FAILED. Check that Visual Studio Build Tools + Windows SDK are installed.
  echo Please send the last error lines to the assistant.
  pause
  exit /b 1
)
copy /Y "target\release\SnowfallGPU.exe" "%~dp0SnowfallGPU.exe" >nul
if errorlevel 1 (
  echo ERROR: Cannot copy the executable.
  pause
  exit /b 1
)
echo.
echo SUCCESS: SnowfallGPU.exe is ready next to this script.
echo You can move SnowfallGPU.exe anywhere; it needs no installer.
pause
