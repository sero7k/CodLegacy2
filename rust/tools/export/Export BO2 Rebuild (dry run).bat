@echo off
rem Shows what the portable export would copy and how big it is; writes nothing.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0export-portable.ps1" -DryRun %*
echo.
pause
