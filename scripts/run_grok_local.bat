@echo off
title Grok2API Local Server
cd /d "E:\AGT_Brain\grok2api_local"
echo Starting Grok2API local server on http://127.0.0.1:8000 ...
.\venv\Scripts\granian.exe --interface asgi --host 127.0.0.1 --port 8000 --workers 1 app.main:app
pause
