@echo off
REM ============================================
REM   Relaunch Antigravity with CDP enabled
REM   for LazyGravity remote control
REM ============================================

echo [1/2] Closing current Antigravity...
taskkill /F /IM Antigravity.exe >nul 2>&1
timeout /t 3 /nobreak >nul

echo [2/2] Launching Antigravity with CDP on port 9333...
start "" "%LOCALAPPDATA%\Programs\Antigravity\Antigravity.exe" --remote-debugging-port=9333

echo.
echo ========================================
echo   Antigravity relaunched with CDP!
echo   Port: 9333
echo   Now run: lazy-gravity start
echo ========================================
echo.
pause
