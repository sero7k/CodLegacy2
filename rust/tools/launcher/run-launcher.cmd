@echo off
rem Builds the player launcher on this PC, puts a "CodLegacy Launcher" shortcut on your Desktop, and opens it.
rem The first build takes a few minutes. The program is left at target\release\codlegacy-launcher.exe.
setlocal
cd /d "%~dp0\..\.."
cargo build --release -p player_launcher || (echo. & echo Build failed - copy the message above and send it back. & pause & exit /b 1)
set EXE=%CD%\target\release\codlegacy-launcher.exe
powershell -NoProfile -ExecutionPolicy Bypass -Command "$s=(New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop')+'\CodLegacy Launcher.lnk'); $s.TargetPath='%EXE%'; $s.WorkingDirectory='%CD%\target\release'; $s.Save()"
start "" "%EXE%"
