@echo off
setlocal
cd /d "%~dp0\.."
set "AHK=%ProgramFiles%\AutoHotkey\v2\AutoHotkey64.exe"
if not exist "%AHK%" set "AHK=%ProgramFiles%\AutoHotkey\v2\AutoHotkey.exe"
if not exist "%AHK%" (
  echo FAIL: AutoHotkey v2 executable was not found. 1>&2
  exit /b 2
)

echo INFO: building current release binaries
cargo build --release --bin uvox-capture --bin uvox-infer --bin uvoxctl || exit /b 1

echo INFO: validating AHK entry points
for %%F in (
  ahk\uvox.ahk
  ahk\tests\hotkeys-manual.ahk
  ahk\tests\ipc-smoke.ahk
  ahk\tests\settings-smoke.ahk
  ahk\tests\typing-smoke.ahk
  ahk\tests\text-transform-smoke.ahk
  ahk\tests\tabprotocol-retry-smoke.ahk
  ahk\tests\full-smoke.ahk
) do (
  echo INFO: validate %%F
  "%AHK%" /ErrorStdOut=UTF-8 /Validate "%%F"
  if errorlevel 1 exit /b 1
)

echo INFO: running AHK smoke tests
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\hotkeys-manual.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\settings-smoke.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\typing-smoke.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\text-transform-smoke.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\tabprotocol-retry-smoke.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\ipc-smoke.ahk" || exit /b 1
"%AHK%" /ErrorStdOut=UTF-8 "ahk\tests\full-smoke.ahk" || exit /b 1

echo PASS: AHK validation and runtime smoke suite
exit /b 0
