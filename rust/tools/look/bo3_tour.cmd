@echo off
rem Claude's BO3 tour: run bo3_tour.ps1 over bo3_maps.txt (three shots per map).
cd /d "%~dp0"
echo running > status_bo3.txt
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0bo3_tour.ps1" > bo3_tour_log.txt 2>&1
echo done %date% %time% > status_bo3.txt
