# Testing matrix

## Checks executed in the source-only editing environment

```bash
python3 tools/ipc-poc/test_poc.py
python3 scripts/verify-static.py
```

The IPC proof of concept passed transport, Unicode, and restart-reconnect checks. Static verification checks the active architecture boundary, AHK v2 directives, required docs, and schema version.

## Rust tests to run on a Windows development machine

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
```

`src/bin/uvox_mock_infer.rs` is a deterministic test-only worker. `tests/worker_lifecycle.rs` launches it as a real child process and covers:

```text
lazy worker launch
warm worker reuse before timeout
idle process exit
worker recycle after model switch
worker crash discard and recovery
blocked inference exact-PID termination fallback
Unicode transcript transport across child pipes
```

The release build script names only `uvox-capture`, `uvox-infer`, and `uvoxctl`, so the mock binary is not staged. The architecture also provides Windows process-level validation through the memory script.

## AHK manual smoke scripts

```powershell
& 'C:\Program Files\AutoHotkey\v2\AutoHotkey.exe' .\ahk\tests\hotkeys-manual.ahk
& 'C:\Program Files\AutoHotkey\v2\AutoHotkey.exe' .\ahk\tests\typing-smoke.ahk
& 'C:\Program Files\AutoHotkey\v2\AutoHotkey.exe' .\ahk\tests\ipc-smoke.ahk
```

Manual checklist:

```text
CapsLock+S hold and release
plain Caps Lock tap
extra modifiers
left and right Ctrl
left and right Alt
AltGr keyboard layout
rapid repeated dictations
runtime hotkey reassignment
disable and re-enable
AHK-generated typing does not re-trigger dictation
foreground-window mismatch prevents typing
Unicode transcript typing
capture-service restart and reconnect
AHK GUI remains responsive during model test/download/transcription
tray commands and stateful enable/disable label
startup shortcut targets shell
```

## Not claimed as executed

The editing environment has no Windows desktop, AutoHotkey runtime, Cargo toolchain, microphone, Parakeet DLL, GGUF model, CUDA runtime, or NVIDIA diagnostics. Rust compilation, AHK execution, and end-to-end audio/model behavior remain target-machine checks.
