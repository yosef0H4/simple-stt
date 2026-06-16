# Simple STT

Simple STT is a Windows-only local dictation application redesigned around a thin AutoHotkey v2 desktop shell, a persistent lightweight Rust audio service, and a disposable Rust Parakeet inference worker.

```text
simple-stt.ahk or simple-stt.exe
  └── simple-stt-capture.exe      persistent CPAL capture + fast overlay
        └── simple-stt-infer.exe  disposable Parakeet DLL/model process

simple-stt-ctl.exe                 disposable AHK control helper
```

The key memory-cleanup guarantee is process exit: `simple-stt-infer.exe` is the only active component allowed to load `parakeet.dll` or a GGUF model. Repeated dictations reuse a warm worker until the configured idle timeout. Cleanup terminates that worker so Windows can reclaim its process RAM and VRAM allocations.

## Start here

Read:

```text
docs/architecture.md
docs/configuration.md
docs/packaging.md
docs/testing.md
docs/memory-cleanup-validation.md
```

On a Windows development machine:

```powershell
.\scripts\bootstrap-dev.ps1
.\scripts\run-dev.ps1 -SkipBuild
```

`bootstrap-dev.ps1` checks or installs Rust, Python, and AutoHotkey v2 when `winget` is available, downloads the prebuilt Parakeet Windows CUDA runtime into the ignored `external\parakeet-runtime\parakeet-windows-cuda\` folder, builds the Rust binaries, and runs source validation. Use `.\scripts\bootstrap-dev.cmd` from Command Prompt.

## Requirements

Manual development setup requires:

```text
Windows 10/11
Rust stable toolchain with Cargo
Python 3.10 or newer
AutoHotkey v2
Parakeet Windows runtime at external\parakeet-runtime\parakeet-windows-cuda\
```

The runtime folder must contain at least:

```text
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

You can use the prebuilt CUDA runtime from:

```text
https://github.com/yosef0H4/parakeet-windows-cuda-build/releases/download/v0.0.1-sm86/parakeet-windows-cuda-sm86.zip
```

Packaging `artifacts\dist\simple-stt-setup.exe` also requires AutoHotkey's Ahk2Exe compiler and Inno Setup 6. NVIDIA GPU/CUDA hardware is recommended for the default `tdt_ctc-110m-f16.gguf`; CPU users should choose `tdt_ctc-110m-q4_k.gguf` from Settings.

## Shell-owned behavior

The AutoHotkey v2 shell owns the tray icon/menu, settings GUI, runtime hold-to-record hotkeys, hotkey recorder, Caps Lock tap behavior, start-with-Windows shortcut, foreground target tracking, transcript transforms, Unicode `SendText()` chunking, clipboard-preserving paste delivery, log opening, user notices, app reload, and exact-PID capture-service supervision.

Text delivery modes are selectable in Settings:

```text
type
paste_ctrl_v
paste_ctrl_shift_v
```

`paste_ctrl_v` is the default. Paste delivery temporarily replaces the clipboard, sends the configured paste shortcut, and restores the full original clipboard with `ClipboardAll()`, including non-text formats such as images and copied objects.

The shell does not parse JSON, move PCM, read a long pipe, load the model, or perform a blocking socket operation from a callback. It starts disposable `simple-stt-ctl.exe` commands and polls completion with `SetTimer()`.

## Rust-owned behavior

`simple-stt-capture.exe` keeps CPAL audio warm, applies gain, downmixes, linearly resamples to 16 kHz mono PCM16, computes overlay levels, buffers only active recordings, owns the rapid Win32 overlay, publishes local control events, and supervises the worker.

When recording begins, `simple-stt-capture.exe` starts warming the speech model in the background. The tooltip reports model loading, loaded, and unloaded lifecycle events. The recording tooltip is a compact stationary Unicode waveform whose bar heights change only with microphone level.

`simple-stt-infer.exe` dynamically loads Parakeet, loads the selected GGUF lazily, handles framed PCM/WAV requests, returns transcripts, and exits after graceful shutdown or its idle backstop. Settings exposes an inference-device dropdown: `nvidia_gpu` uses the bundled CUDA backend and `cpu` forces CPU inference without VRAM use. The settings UI also separates installed-model selection from the downloadable catalog, uses a microphone dropdown instead of a free-text box, and shows absolute runtime/model paths.

## Testing

### Run the complete Windows validation suite

From Command Prompt or PowerShell:

```bat
scripts\test-full.cmd
```

This is the main pre-commit command. It runs:

```text
cargo test --all-targets
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
scripts\test-ahk-full.cmd
```

The full suite builds current release binaries before running the AutoHotkey smoke tests, so stale `target\release` executables cannot hide source changes.

### What the complete suite covers

The combined suite verifies:

```text
Rust unit tests and real child-process worker lifecycle tests
schema-v2 config round trips and schema-1 migration backup
Windows Common Controls v6 manifest embedding for modern tooltips
capture-service / inference-worker process boundaries
loopback authenticated IPC, Unicode transport, and reconnect
AutoHotkey v2 syntax validation for the shell and every AHK test entry point
settings GUI open/save persistence
hotkey parsing and binding behavior
foreground-window-safe typed delivery
casual-text transforms: punctuation removal and lowercase conversion
response-file sharing-violation retry handling
real Parakeet model load and model-test transcription
worker unload and PID disappearance
recording-start model prewarm before transcription
capture-service restart and reconnect
single `hello world` typed-delivery check
`Ctrl+V` and `Ctrl+Shift+V` paste delivery checks
full non-text clipboard-format restoration after paste
```

The end-to-end smoke uses an isolated temporary config and state directory. It does not overwrite the normal `%APPDATA%\simple-stt\config.json` file or reuse the live shell discovery file.

The paste smoke intentionally sends only `hello world` into controlled temporary edit boxes. It also places a custom non-text object format on the clipboard before pasting and asserts that the object format is restored afterward.

### Run the settings GUI preview harness

When editing `ahk/lib/SettingsGui.ahk`, validate with the console-first preview harness after each small batch so AHK syntax/runtime errors surface on stderr instead of as modal GUI popups:

```bat
python scripts\run-settings-preview.py
```

The launcher first runs AutoHotkey `/Validate`, then opens the isolated preview loop in `ahk\tests\settings-preview.ahk`.

Artifacts:

```text
artifacts\gui-loop\report.txt
artifacts\gui-loop\default-*.png
artifacts\gui-loop\compact-*.png
artifacts\gui-loop\wide-*.png
artifacts\gui-loop\final-general.png
```

A passing run reports `RESULT: PASS` and `screenshots: 13`. The preview loop safely exercises every settings button callback, save/reload flow, mock IPC path, and screenshot capture path without opening the real app or relying on live services.

### Run only the AutoHotkey validation and runtime smoke suite

```bat
scripts\test-ahk-full.cmd
```

This command:

```text
builds simple-stt-capture, simple-stt-infer, and simple-stt-ctl in release mode
runs AutoHotkey /Validate with /ErrorStdOut=UTF-8 for every entry point
runs the AHK smoke scripts
runs the real model load/unload/prewarm and paste-delivery E2E smoke
```

The runner looks for AutoHotkey v2 at:

```text
%ProgramFiles%\AutoHotkey\v2\AutoHotkey64.exe
%ProgramFiles%\AutoHotkey\v2\AutoHotkey.exe
```

Load-time AHK failures are written to stderr instead of GUI dialogs.

### Run source-only checks

```powershell
.\scripts\test-static.ps1
```

Or run the underlying commands directly if PowerShell execution policy blocks unsigned local scripts:

```bat
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
```

These checks do not require the speech model. They verify source invariants and the loopback IPC proof of concept.

### Run Rust tests only

```powershell
cargo test --all-targets
```

This includes deterministic worker lifecycle integration tests using the test-only `simple_stt_mock_infer` child process. The mock worker is never included in release packaging.

### Manual release checks still required

Automation is intentionally strong, but it does not replace a final desktop pass. Before publishing a release, manually verify:

```text
physical hold-to-record microphone dictation
Caps Lock tap behavior and configured record chord
real overlay placement near the cursor on each monitor
compact stationary waveform appearance
model loaded and unloaded tooltip notices
tray Reload App action
startup shortcut creation and removal
microphone selection followed by audio-service restart
typing and paste delivery into representative target applications
RAM and VRAM cleanup after repeated unload and idle-timeout cycles
packaging with build-distribution.cmd
```

See `docs/testing.md` and `docs/memory-cleanup-validation.md` for the detailed matrix.

## Legacy source

The retired monolith is no longer checked into the working tree. Use git history if you need to inspect the old structure.

## Install-relative paths

Relative runtime/model directories resolve from the checkout root for Cargo `target\debug` / `target\release` binaries and from the executable directory for packaged binaries. Persisted config, logs, state, and cached catalog data are isolated per runtime root so portable, installed, and checkout/dev builds do not accidentally share the wrong model or runtime location. `build-distribution.cmd` and `scripts\build-distribution.ps1` stage `fixtures\parakeet-smoke.wav` for model testing.

The source repository intentionally does not vendor Parakeet runtime binaries, GGUF model files, AutoHotkey installs, or unrelated external reference projects. Place a compatible Parakeet runtime under `external\parakeet-runtime\parakeet-windows-cuda\` for local development or packaging, and review upstream licenses before publishing any binary package that includes runtime/model artifacts.

The default `build-distribution.cmd` installer includes the Parakeet runtime DLLs but does not embed a GGUF model. It offers a default-checked installer task to download the recommended model from Hugging Face; if that fails, installation continues and users can download a model later from Settings. Use `build-distribution.cmd -IncludeModel` only for an explicit offline/private bundle.

## License

GPL-2.0-only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.
