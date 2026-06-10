# Testing matrix

## Authoritative Windows validation command

Run this before committing code changes:

```bat
scripts\test-full.cmd
```

The command runs:

```text
cargo test --all-targets
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
scripts\test-ahk-full.cmd
```

The AutoHotkey portion rebuilds current release binaries first, so runtime smoke tests cannot accidentally validate stale executables.

## Full-suite coverage

The combined validation suite covers:

```text
Rust unit tests
real-child-process worker lifecycle integration tests
schema-v2 config validation and schema-1 migration backup
install-relative runtime path behavior
Windows Common Controls v6 manifest embedding
loopback authenticated IPC
Unicode transport and malformed protocol rejection
state-file reconnect after simulated service restart
AHK v2 load validation for all shell and test entry points
settings GUI open/save persistence
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

## Run Rust tests only

```powershell
cargo test --all-targets
```

Included Rust unit coverage:

```text
audio mono/downmix/resampling/frame behavior
shell JSON Unicode and malformed JSON
escaped helper protocol Unicode/control-character round trip
worker framed protocol PCM and Unicode transcript framing
protocol-version and malformed-size rejection
schema-v2 validation and schema-1 migration backup
approved model-name restriction
lazy launch / warm reuse / model replacement / idle policy
install-relative runtime root behavior
worker logging-level propagation
stationary compact Unicode waveform behavior
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

The release build script names only `simple-stt-capture`, `simple-stt-infer`, and `simple-stt-ctl`, so the mock binary is not staged.

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
schema migration, atomic writes, and download hygiene
mock-worker exclusion from release packaging
```

The IPC proof of concept validates:

```text
authenticated loopback PING/PONG
START_RECORDING / STOP_RECORDING
asynchronous polled Unicode TRANSCRIPT
state-file reconnect after simulated service restart
```

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
ahk\tests\settings-smoke.ahk
ahk\tests\typing-smoke.ahk
ahk\tests\text-transform-smoke.ahk
ahk\tests\tabprotocol-retry-smoke.ahk
ahk\tests\full-smoke.ahk
```

Runtime smoke scripts:

```text
hotkeys-manual.ahk
  parser and runtime binding smoke

settings-smoke.ahk
  real GUI construction and Save persistence through simple-stt-ctl
  text-delivery mode and casual-text option persistence

typing-smoke.ahk
  typed queue behavior and foreground mismatch cancellation

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
release packaging with scripts\package-release.ps1
```

For memory-specific measurements, see `docs/memory-cleanup-validation.md`.
