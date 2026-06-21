@echo off
REM ===== Antigravity Brain — Daily Cron =====
REM Chạy brain-cron mỗi lần khởi động hoặc scheduled task
REM Cách dùng:
REM   1. Chạy trực tiếp: double-click file này
REM   2. Schedule: Task Scheduler > Create Task > Action: Start Program > Browse to this .bat
REM      Trigger: Daily at 23:00

set AGT_BRAIN_ROOT=E:\AGT_Brain
cd /d E:\AGT_Brain

echo [%date% %time%] Running Antigravity Brain Cron...
echo ================================================

REM One-shot reflection
target\debug\brain-cron.exe

echo.
echo ================================================
echo [%date% %time%] Done!
echo.
pause
