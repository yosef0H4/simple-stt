@echo off
setlocal
cd /d "%~dp0\.."

echo INFO: running Rust tests
cargo test --all-targets || exit /b 1

echo INFO: running static architecture checks
python scripts\verify-static.py || exit /b 1
python tools\ipc-poc\test_poc.py || exit /b 1

echo INFO: running AutoHotkey validation and runtime smoke
call scripts\test-ahk-full.cmd || exit /b 1

echo PASS: full SimpleStt validation suite
exit /b 0
