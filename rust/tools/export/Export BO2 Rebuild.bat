@echo off
rem Makes the portable BO2 Rebuild folder (see docs\EXPORT.md). Arguments pass through,
rem e.g. -Destination E:\BO2_Rebuild -NoBuild -SkipVerify -VerifyMaps.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0export-portable.ps1" %*
echo.
pause
