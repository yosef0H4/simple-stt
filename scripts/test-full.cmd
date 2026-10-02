@echo off
setlocal
cd /d "%~dp0\.."

echo INFO: building and verifying Settings frontend assets
where npm >nul 2>nul || (echo ERROR: npm was not found. Install Node.js. & exit /b 1)
pushd web\settings
call npm ci || exit /b 1
call npm run build || exit /b 1
popd
call python scripts\verify-settings-frontend.py || exit /b 1

echo INFO: running Rust tests
cargo test --all-targets || exit /b 1

cargo build --bin simple-stt-settings || exit /b 1
call python scripts\test-settings-selection-api.py || exit /b 1

echo INFO: running Settings cleanup end-to-end test
call python scripts\test-cleanup-settings-e2e.py || exit /b 1

echo INFO: running static architecture checks
call python scripts\verify-static.py || exit /b 1
call python tools\ipc-poc\test_poc.py || exit /b 1

echo INFO: running Settings browser regressions (Playwright must be available)
node scripts\test-settings-model-selection.cjs || exit /b 1
node scripts\test-settings-overhaul.cjs || exit /b 1
node scripts\test-settings-contracts.cjs || exit /b 1

echo INFO: running AutoHotkey validation and runtime smoke
call scripts\test-ahk-full.cmd || exit /b 1

echo PASS: full SimpleStt validation suite
exit /b 0
