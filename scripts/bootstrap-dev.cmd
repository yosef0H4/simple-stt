@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0bootstrap-dev.ps1" %*
exit /b %ERRORLEVEL%
