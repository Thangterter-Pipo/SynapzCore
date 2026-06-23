@echo off
REM ============================================
REM   Relaunch Antigravity with CDP enabled
REM   for SynapzCore CDP Controller remote control
REM ============================================

echo [1/2] Closing current Antigravity...
taskkill /F /IM "Antigravity IDE.exe" >nul 2>&1
taskkill /F /IM "Antigravity.exe" >nul 2>&1
timeout /t 3 /nobreak >nul

echo [2/2] Launching Antigravity with CDP on port 9333...
start "" "%LOCALAPPDATA%\Programs\Antigravity IDE\Antigravity IDE.exe" --remote-debugging-port=9333

echo.
echo ========================================
echo   Antigravity relaunched with CDP!
echo   Port: 9333
echo   CDP Controller is ready to connect.
echo ========================================
echo.
pause
