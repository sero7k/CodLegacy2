@echo off
rem Builds CodLegacy-Setup.exe. Needs Rust and NSIS (winget install NSIS.NSIS).
rem Put the public signing key in crates\player_launcher\update_key.pub first (see docs\LAUNCHER_ACCOUNTS.md).
setlocal
cd /d "%~dp0\..\.."
for /f "tokens=3 delims= " %%v in ('findstr /b /c:"version" crates\player_launcher\Cargo.toml') do set VER=%%~v
cargo build --release -p player_launcher || exit /b 1
set NSIS="%ProgramFiles(x86)%\NSIS\makensis.exe"
if not exist %NSIS% set NSIS="%ProgramFiles%\NSIS\makensis.exe"
%NSIS% /DVERSION=%VER% /DEXE="%CD%\target\release\codlegacy-launcher.exe" /DOUT="%CD%\CodLegacy-Setup.exe" tools\installer\codlegacy.nsi || exit /b 1
echo Built %CD%\CodLegacy-Setup.exe (version %VER%)
