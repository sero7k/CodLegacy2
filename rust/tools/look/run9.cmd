@echo off
rem Claude's all-maps look round: build origin/main (run4.ps1), then run9.ps1 over maps9.txt.
cd /d "%~dp0"
if not exist out4 mkdir out4
if not exist out9 mkdir out9
echo building > out9\status.txt
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run4.ps1" > out4\log.txt 2>&1
echo running > out9\status.txt
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run9.ps1" > out9\log.txt 2>&1
echo done %date% %time% > out9\status.txt
