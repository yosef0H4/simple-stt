# Uvox STT

Uvox is a lightweight Windows dictation utility built around native Rust desktop integration and a local CUDA Parakeet runtime.

Hold **CapsLock** to record. Release **CapsLock** to transcribe the completed clip with native Parakeet and type the result into the original focused application.

Uvox does not try to bypass applications that reject injected input. Text is inserted with normal Win32 Unicode `SendInput` events at a fixed configurable pace.

## Current Design

```text
CapsLock down
→ Rust records microphone audio immediately
→ native Parakeet CUDA runtime loads or is reused
→ CapsLock release stops recording
→ completed 16 kHz mono PCM16 clip is transcribed
→ Rust types the transcript into the original focused app
→ idle timeout unloads Parakeet to free memory
```

There is no Python worker, C# helper, browser shell, live translation, or live partial transcription path.

`uvox run` is a resident tray utility. The tray menu opens settings, disables/enables the CapsLock hotkey, reloads config, opens the latest log, tests the model, and exits the app.

## Prerequisites

- Windows 10 or newer
- NVIDIA CUDA-capable GPU and current NVIDIA driver
- Rust stable with Cargo
- PowerShell
- The local native Parakeet runtime extracted at:

```text
external\parakeet-runtime\parakeet-windows-cuda
```

Required runtime files:

```text
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

The runtime bundle is intentionally ignored by git because it is large.

## First Test

From PowerShell at the repository root:

```powershell
.\scripts\check-prereqs.ps1
.\scripts\test-audio.ps1
```

The audio smoke test uses `tests\fixtures\parakeet-smoke.wav`. A good transcript starts with:

```text
Well, I don't wish to see it any more
```

The Parakeet runtime should log that it is using CUDA.

## Run

```powershell
.\scripts\run.ps1
```

Focus a normal text box, hold CapsLock while speaking, then release CapsLock to transcribe and type.

`scripts\run-dev.ps1` is kept as a compatibility alias for `scripts\run.ps1`.

## Useful Commands

```powershell
cargo run -p uvox -- run
cargo run -p uvox -- transcribe-file --audio tests\fixtures\parakeet-smoke.wav
cargo run -p uvox -- list-inputs
cargo run -p uvox -- config-show
cargo run -p uvox -- config-reset
cargo run -p uvox -- settings
cargo run -p uvox -- ui-screenshot --surface settings --output artifacts\ui-settings.png
cargo test -p uvox
```

## Settings

The config file is stored at the platform config path unless `UVOX_CONFIG` is set. It controls microphone matching, gain, typing pace, idle unload timeout, and Parakeet runtime/model paths.

Open it with:

```powershell
.\scripts\settings.ps1
```

The settings window is native Win32 and organized by General, Audio, Model, Typing, Logging, and Advanced sections. It includes startup toggle support, latest-log access, a model test button, and a recommended-model download/test/select action.

## Logs

Uvox writes one latest-run log file and replaces it on each launch:

```powershell
cargo run -p uvox -- config-show
```

The actual log path is under the user local app-data directory as `uvox\latest.log`. Settings and tray can open it directly. Logging levels are `minimal`, `normal`, `debug`, and `extreme`.

## UI Screenshots

Agents can render deterministic UI screenshots while iterating on layout:

```powershell
cargo run -p uvox -- ui-screenshot --surface settings --section audio --output artifacts\ui-settings-audio.png
cargo run -p uvox -- ui-screenshot --surface overlay --output artifacts\ui-overlay.png
```

## Repository Map

```text
rust/                 native Windows app and Parakeet FFI
scripts/              PowerShell launch/test helpers
tests/fixtures/       retained audio smoke-test fixture
docs/                 native architecture/debugging notes
AGENTS.md             coding-agent entrypoint
THIRD_PARTY_NOTICES.md third-party notes
```

## License

The repository is released under GPL-2.0-only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.
