# Agent notes

Read these files before editing:

```text
docs/architecture.md
docs/ahk-v2-research.md
docs/current-behavior-inventory.md
docs/testing.md
docs/memory-cleanup-validation.md
```

## Hard architecture boundaries

```text
AHK owns tray, GUI, hotkeys, startup registration, target-window safety,
text transforms, final typing, clipboard-preserving paste delivery, and app reload.

simple-stt-capture owns CPAL audio, the fast Win32 overlay, shell IPC, downloads,
recording-start model prewarm, lifecycle tooltip notices, and worker supervision.

simple-stt-infer is the only active process allowed to load parakeet.dll or GGUF models.
```

Do not reintroduce in-process Parakeet loading under `src/capture/`.

Do not remove the Windows resource build pipeline:

```text
build.rs
resources\windows.rc
resources\simple-stt.exe.manifest
```

The manifest activates Common Controls v6 so Win32 tooltips use modern Windows visual styles.

## AutoHotkey v2 rules

Do not copy AHK v1 syntax into `ahk/`. Every executable AHK entry point starts with:

```ahk
#Requires AutoHotkey v2.0
#SingleInstance Force
```

Be careful with AHK v2 name shadowing. Do not name locals after constructors or classes on the same assignment line. For example, avoid:

```ahk
gui := Gui(...)
typist := Typist(...)
```

Use names such as:

```ahk
window := Gui(...)
typistInstance := Typist(...)
```

AHK helper response files can briefly produce Windows sharing violation error 32. Keep the bounded retry in `ahk\lib\TabProtocol.ahk` and its regression smoke test.

Paste delivery must preserve the complete clipboard, not only text. Keep the `ClipboardAll()` backup, guarded restore, clipboard sequence-number check, and the non-text clipboard-format E2E assertion.

## Required pre-commit validation

Run the complete Windows validation suite before committing code changes:

```bat
scripts\test-full.cmd
```

This is the authoritative local validation command. It runs:

```text
cargo test --all-targets
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
scripts\test-ahk-full.cmd
```

`scripts\test-ahk-full.cmd` rebuilds the current release binaries before executing AHK smoke tests. This is intentional: do not validate AHK behavior against stale `target\release` binaries.

## Targeted test commands

Run only Rust tests:

```powershell
cargo test --all-targets
```

Run only source and IPC checks:

```powershell
.\scripts\test-static.ps1
```

If PowerShell execution policy blocks unsigned scripts, use:

```bat
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
```

Run only AHK validation and runtime smoke tests:

```bat
scripts\test-ahk-full.cmd
```

## AutoHotkey suite coverage

The AHK runner validates every entry point with:

```text
/ErrorStdOut=UTF-8 /Validate
```

This catches load-time AHK errors without modal GUI error dialogs.

It then runs:

```text
ahk\tests\hotkeys-manual.ahk
ahk\tests\settings-smoke.ahk
ahk\tests\typing-smoke.ahk
ahk\tests\text-transform-smoke.ahk
ahk\tests\tabprotocol-retry-smoke.ahk
ahk\tests\ipc-smoke.ahk
ahk\tests\full-smoke.ahk
```

The full AHK smoke uses an isolated temporary config and state file and covers:

```text
capture-service startup and authenticated ping
microphone and model listing
real model-test transcription
worker unload and PID disappearance
recording-start model prewarm before transcription
capture-service restart and reconnect
one controlled hello world typed-delivery check
Ctrl+V paste delivery
Ctrl+Shift+V paste delivery
restoration of a custom non-text clipboard object format
```

Do not expand the typing test into a large keyboard simulation. `hello world` is deliberately enough to exercise the normal path without causing unnecessary input activity.

## Rust suite coverage

`cargo test --all-targets` includes unit tests and the real-child-process lifecycle suite in `tests\worker_lifecycle.rs`.

The lifecycle tests cover:

```text
lazy worker launch
warm reuse
idle exit
model-switch recycle
crash recovery
blocked inference exact-PID forced termination
Unicode transcript transport
```

The mock worker is test-only. Do not ship `simple_stt_mock_infer` in release packaging.

## Static verifier responsibilities

`scripts\verify-static.py` protects invariants that are easy to regress during refactors, including:

```text
AHK v2 entry directives
split Rust binary architecture
Parakeet isolation in simple-stt-infer
Common Controls v6 manifest embedding
loopback authenticated IPC boundaries
clipboard-preserving paste implementation
TabProtocol sharing-violation retry
recording-start worker warm-up
release lockfile hygiene
mock-worker exclusion from release packaging
```

When adding an architectural feature, extend the static verifier and the relevant runtime smoke test together.

## Manual release checks

Automation is not a substitute for a final desktop pass. Before publishing a release, manually verify:

```text
physical microphone dictation
configured hold-to-record chord and Caps Lock tap behavior
compact stationary waveform appearance
model loaded and unloaded tooltip notices
tray Reload App action
startup shortcut creation/removal
microphone switch followed by audio-service restart
typing and paste delivery into representative applications
RAM and VRAM cleanup after unload and idle timeout
release packaging
```

Use git history as the reference for old monolith behavior; the archived monolith tree is no longer present in the working tree.
