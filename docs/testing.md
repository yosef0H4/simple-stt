# Testing matrix

## Pre-commit validation

Run validation for the current platform before committing code changes. On both
Linux and Windows, run formatting, Clippy, Rust tests, Settings API tests, cleanup
E2E, static verification, and IPC tests:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --bin simple-stt-settings
python scripts/test-settings-selection-api.py
python scripts/test-cleanup-settings-e2e.py
python scripts/verify-static.py
python tools/ipc-poc/test_poc.py
```

Build the Settings frontend before any Cargo build. Release packaging and the
full-suite scripts run this automatically. Linux uses `scripts/test-full.sh`
(with Playwright available) and `scripts/build-linux-release.sh`. During targeted
development, run `cd web/settings && npm ci && npm run build`. The build writes
`dist/index.html`, `dist/app.js`, `dist/styles.css`, and a deterministic
`dist/manifest.json`. `python scripts/verify-settings-frontend.py` checks source
and output hashes plus the 100 KiB gzipped JS/CSS and 300 KiB total asset budgets.
The Rust build script independently rejects missing or stale frontend output.

On Linux, also run `python scripts/test-linux-static.py`. Run the browser model
selection regression for Settings changes and the relevant real-device tests
below for keyboard routing or inference changes. Cross-compile and run Clippy
for Windows-specific changes when the Windows target toolchain is available.

On Windows, the authoritative native validation command is:

```bat
scripts\test-full.cmd
```

The command runs:

```text
cargo test --all-targets
cargo build --bin simple-stt-settings
python scripts\test-settings-selection-api.py
python scripts\test-cleanup-settings-e2e.py
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
node scripts\test-settings-model-selection.cjs
node scripts\test-settings-overhaul.cjs
node scripts\test-settings-contracts.cjs
scripts\test-ahk-full.cmd
```

The AutoHotkey portion rebuilds current release binaries first, so runtime smoke tests cannot accidentally validate stale executables.

If a Windows host is unavailable, report native Windows runtime checks as
unvalidated. They do not block committing changes validated on Linux.

The cleanup E2E launches the real `simple-stt-settings` process against a
deterministic OpenAI-compatible HTTP server, then exercises bootstrap and
**Test cleanup** across the authenticated browser API. Debug-only provider
values can be supplied through the ignored `.env` file documented by
`.env.example`.

## Full-suite coverage

The combined validation suite covers:

```text
Rust unit tests
real-child-process worker lifecycle integration tests
nested schema-v9 normalization and malformed-file preservation
AI cleanup provider response parsing, Unicode preservation, and raw-text fallback
AI credential separation from portable configuration
install-relative runtime path behavior
Windows Common Controls v6 manifest embedding
loopback authenticated IPC
Unicode transport and malformed protocol rejection
state-file reconnect after simulated service restart
AHK v2 load validation for all shell and test entry points
authenticated browser Settings load/save and external-edit conflict detection
typed delivery and foreground mismatch cancellation
punctuation removal and lowercase transcript transforms
Windows response-file sharing-violation retry handling
real Parakeet model-test transcription
worker unload and PID disappearance
recording-start model prewarm while recording remains active
capture-service restart and reconnect
Ctrl+V paste delivery
Ctrl+Shift+V paste delivery
restoration of a custom non-text clipboard format
```

The end-to-end smoke uses isolated temporary config and state files. It does not overwrite `%APPDATA%\simple-stt\config.json` or reuse the live shell discovery file.

The Windows AHK full smoke also uses a controlled edit window to switch the foreground thread between installed English and Arabic keyboard layouts. It checks the recording-start language event, runs the Arabic model test, and restores the original layout. The native inference check runs both English and Arabic models on CPU and Vulkan:

```powershell
python scripts/test-inference-devices.py --mode both --language both
```

The saved Arabic fixtures include four synthetic prompts and three human FLEURS clips. Conversion parity was checked locally before publishing the finished Q8 GGUF. Checkpoints, conversion environments, and NeMo/F16 comparison scripts are not part of this repository or required for installation.

For experimental memory and latency comparisons across 50 alternating switches:

```powershell
python scripts/benchmark-language-switch.py --switches 50
```

On Linux, run `python scripts/test-linux-static.py` alongside the normal Rust tests. Follow-keyboard selection supports KDE Plasma Wayland and X11. Fixed English and Arabic modes work on all supported Linux desktops. Install the Linux Vulkan runtime and both models with `bash scripts/bootstrap-linux-vulkan.sh`, then run `python scripts/test-inference-devices.py --mode both --language both`. To check a live layout, run `SIMPLE_STT_TEST_KEYBOARD_LANGUAGE=english cargo test --lib live_keyboard_language` (or `arabic` after switching layouts).

## Run Rust tests only

```powershell
cargo test --all-targets
```

Included Rust unit coverage:

```text
audio mono/downmix/resampling/frame behavior
preferred microphone fallback/return, Windows endpoint-event retry policy, and default tracking
shell JSON Unicode and malformed JSON
escaped helper protocol Unicode/control-character round trip
worker framed protocol PCM and Unicode transcript framing
protocol-version and malformed-size rejection
schema-v9 normalization, unknown-field removal, and malformed-file preservation
AI cleanup defaults, nested invalid-value recovery, OAuth PKCE, and Codex SSE parsing
approved model-name restriction
lazy launch / warm reuse / model replacement / idle policy
install-relative runtime root behavior
worker logging-level propagation
stationary compact Unicode waveform behavior
new-recording supersession and delivery-completion acknowledgement
```

`src/bin/simple_stt_mock_infer.rs` is a deterministic test-only worker. `tests/worker_lifecycle.rs` launches it as a real child process and covers:

```text
lazy worker launch
warm worker reuse before timeout
idle process exit
worker recycle after model switch
worker crash discard and recovery
blocked inference exact-PID termination fallback
Unicode transcript transport across child pipes
```

The release build script names `simple-stt-capture`, `simple-stt-infer`, `simple-stt-ctl`, and `simple-stt-settings`, so the mock binary is not staged.

## Run source and IPC checks only

```powershell
.\scripts\test-static.ps1
```

If local PowerShell execution policy blocks unsigned scripts, invoke the underlying checks directly:

```bat
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
```

The static verifier checks architectural invariants that are easy to regress during refactors:

```text
AHK v2 directives on every executable entry point
split Rust binary structure
Parakeet DLL isolation in simple-stt-infer
Common Controls v6 manifest embedding for modern tooltips
loopback-only versioned control IPC
framed Rust-to-Rust PCM protocol
clipboard-preserving paste implementation
response-file sharing-violation retry
recording-start worker warm-up
schema normalization, atomic writes, authenticated Settings, and download hygiene
mock-worker exclusion from release packaging
```

The IPC proof of concept validates:

```text
authenticated loopback PING/PONG
START_RECORDING / STOP_RECORDING
asynchronous polled Unicode TRANSCRIPT
state-file reconnect after simulated service restart
```

## Browser Settings coverage

Browser automation covers all six pages, including AI Cleanup, narrow and wide layouts, accessible labels, explicit Save, Reset/Import previews, security headers, offline editing, device/model controls, provider model search/manual IDs, credential-state controls, and download progress. Release validation additionally starts the capture service, downloads a real catalog model through the UI, waits for completion, selects and saves it, and runs the real smoke-audio model test. The routine CI download uses a deterministic local fixture server; the release pass uses the production catalog and model URL.

Provider tests use deterministic loopback fixtures and must never use a developer's saved API key. A real provider test is manual, requires a freshly issued credential supplied through the OS vault or `SIMPLE_STT_AI_API_KEY`, and must verify that a failed or timed-out request delivers the original transcript. Screenshot testing must also verify the visible capture notice, denylist behavior, active-window targeting, and that no image or history survives capture-process exit. Wayland's compositor-owned picker remains a manual desktop check.

For an opt-in quality check against the provider configured in the ignored local `.env`, run:

```powershell
python scripts\benchmark-cleanup-live.py
```

The benchmark uses the real debug Settings process and scores several unrelated ASR-error patterns plus a preservation control. It prints provider output for review and is intentionally excluded from deterministic validation because model output and external service availability can vary.

## Linux X11 shortcut E2E

The native X11 shortcut listener has an opt-in end-to-end test. It needs `Xvfb`, `xdpyinfo`, and `xdotool`:

```bash
Xvfb :99 -screen 0 1024x768x24 -nolisten tcp
DISPLAY=:99 SIMPLE_STT_X11_E2E=1 cargo test --bin simple-stt-linux x11_global_shortcut_end_to_end -- --nocapture
```

The test registers `Meta+Z` through the production X11 parser and passive-grab path, synthesizes the chord with `xdotool`, and requires the corresponding X11 key event. The ordinary test suite skips external X-server interaction unless `SIMPLE_STT_X11_E2E` is set.

## Run AutoHotkey validation and runtime smoke only

```bat
scripts\test-ahk-full.cmd
```

The runner resolves AutoHotkey v2 from:

```text
%ProgramFiles%\AutoHotkey\v2\AutoHotkey64.exe
%ProgramFiles%\AutoHotkey\v2\AutoHotkey.exe
```

It first runs load-time validation with:

```text
/ErrorStdOut=UTF-8 /Validate
```

That sends AHK load errors to stderr instead of opening modal GUI error dialogs.

Validated entry points:

```text
ahk\simple-stt.ahk
ahk\tests\hotkeys-manual.ahk
ahk\tests\ipc-smoke.ahk
ahk\tests\typing-smoke.ahk
ahk\tests\text-transform-smoke.ahk
ahk\tests\tabprotocol-retry-smoke.ahk
ahk\tests\full-smoke.ahk
```

Runtime smoke scripts:

```text
hotkeys-manual.ahk
  parser and runtime binding smoke

typing-smoke.ahk
  typed queue behavior, foreground mismatch cancellation, and completion acknowledgement

text-transform-smoke.ahk
  punctuation removal, lowercase conversion, and combined transform

tabprotocol-retry-smoke.ahk
  reproduces an exclusive Windows file lock and verifies bounded read retry

ipc-smoke.ahk
  authenticated capture-service ping and graceful shutdown

full-smoke.ahk
  isolated capture service, microphone/model listing, real model test,
  unload verification, recording-start prewarm, restart/reconnect,
  one hello world typed check, Ctrl+V paste, Ctrl+Shift+V paste,
  and custom non-text clipboard-format restoration
```

The typing and paste checks intentionally use only `hello world` in controlled temporary edit boxes. Do not turn them into large keyboard simulations.

## Manual release checklist

Automation does not replace a final desktop pass. Before publishing a release, manually verify:

```text
physical microphone capture and transcription
CapsLock+S hold and release
plain Caps Lock tap
extra modifiers, left/right Ctrl, left/right Alt, and AltGr layouts
rapid repeated dictations
runtime hotkey reassignment
disable and re-enable hotkey
foreground-window mismatch prevents delivery
Unicode transcript delivery
capture-service restart and reconnect
AHK GUI responsiveness during model test/download/transcription
tray Reload App action
tray commands and stateful enable/disable label
startup shortcut creation and removal
microphone selection followed by audio-service restart
compact stationary waveform placement near the cursor on each monitor
model loading, loaded, and unloaded tooltip notices
RAM and VRAM cleanup after explicit unload and idle timeout
release packaging with build-distribution.cmd
```

For memory-specific measurements, see `docs/memory-cleanup-validation.md`.

Linux desktop regression: `python scripts/test-linux-language.py --switch-layouts` briefly switches configured KDE English/Arabic layouts, verifies recording-start selection through authenticated capture IPC, tests both real models, unloads the worker, and restores the layout. For NVIDIA verification use `python scripts/test-inference-devices.py --mode both --language both --expect-gpu "NVIDIA GeForce RTX 3050 Ti"`; it requires the native selected-device log and an exact worker PID with NVIDIA VRAM allocation.

Model selection regressions: `python scripts/test-settings-selection-api.py` exercises the real Settings HTTP server's schema migration, nullable selections, authenticated read-only discovery, unavailable Wayland, Save, and draft-only reset/import. Build the debug Settings binary first with `cargo build --bin simple-stt-settings`. `node scripts/test-settings-model-selection.cjs` exercises the browser UI using Playwright; install Playwright or set `NODE_PATH` to the bundled Node packages. It tests searchable mouse/keyboard selection, deterministic matches, preserved None, unavailable files, saved removal guards, download events, Save/Reset/import, and narrow layouts.

The Linux language regression also checks silent skipped starts, real shell toggle/stop state and clipboard preservation, and worker PID reuse when English and Arabic share a model. The Windows full smoke checks skipped starts and explicitly selects its installed fixture models; run `scripts\test-full.cmd` when validating on Windows. Windows cross-compilation verifies build compatibility but does not replace native keyboard, AHK, or Vulkan desktop tests; report those checks as unvalidated when no Windows host is available.


Settings overhaul browser validation: `node scripts/test-settings-overhaul.cjs`
covers every page, cross-page drafts, Save/Reset, shortcut capture, delivery
cycles and app overrides, cleanup testing, credential/OAuth actions against
local fixtures, metadata search, JSON editing/export, conflicts, offline editing,
and Linux-specific controls. `node scripts/test-settings-contracts.cjs` checks
TypeScript contracts against the real Rust API and loads the compiled UI under
its CSP. These scripts require Node with TypeScript type stripping support
(Node 24 is tested) and Playwright through `NODE_PATH` or an installation.

For release-asset performance comparisons run `node scripts/benchmark-settings.cjs`
with `SETTINGS_WEB_ROOT` pointing to baseline assets, then current assets. Use
`SETTINGS_BENCHMARK_OUTPUT` to retain measurements and
`SETTINGS_BENCHMARK_BASELINE` on the current pass to enforce startup/interaction
and heap budgets. Linux Settings-server RSS comparison uses
`python scripts/benchmark-settings-server.py --baseline PATH --current PATH`.
These tests use isolated configurations and never modify the running app's
settings. Browser heap measures only the tab's JavaScript; shared browser engine
RAM is separate. Background shell/capture code is outside this UI overhaul.
