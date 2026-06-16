@echo off
setlocal
set AHK=C:\PROGRA~1\AutoHotkey\v2\AutoHotkey64.exe
set SCRIPT=%~dp0..\ahk\tests\settings-preview.ahk
%AHK% /ErrorStdOut=UTF-8 /Validate %SCRIPT%
if errorlevel 1 exit /b %errorlevel%
%AHK% /ErrorStdOut=UTF-8 %SCRIPT%
exit /b %errorlevel%
