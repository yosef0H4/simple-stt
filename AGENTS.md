# AGENTS.md

This file is the coding-agent entrypoint for Uvox.

## Mission

Keep Uvox a lightweight Windows-first local dictation utility:

```text
CapsLock press
→ Rust starts retaining microphone audio immediately
→ native CUDA Parakeet runtime loads or is reused
→ CapsLock release stops recording
→ Rust transcribes the complete clip in-process through parakeet.dll
→ Rust inserts transcript text into the original focused app
→ idle timeout unloads Parakeet
```

Do not add Python, C#, browser-based desktop frameworks, live translation, or live partial transcription paths.

## Safety and Product Rule

Do not add random delays, fake mistakes, or anti-detection behavior. Fixed configurable text pacing exists for normal UX and compatibility. Respect applications that reject injected input.

Never create a git commit unless the user's latest message explicitly asks for a commit.

## First Debugging Commands

Run in this order:

```powershell
.\scripts\check-prereqs.ps1
.\scripts\test-audio.ps1
cargo test -p uvox
cargo run -p uvox -- list-inputs
cargo run -p uvox -- ui-screenshot --surface settings --output artifacts\ui-settings.png
cargo run -p uvox -- ui-screenshot --surface overlay --output artifacts\ui-overlay.png
cargo run -p uvox -- run
```

## Key Modules

| Module | Responsibility |
|---|---|
| `rust/src/audio.rs` | CPAL input stream and 16 kHz mono PCM16 frame delivery |
| `rust/src/resample.rs` | downmix, gain, linear resampling, PCM framing |
| `rust/src/hotkey.rs` | global low-level CapsLock down/up hook and suppression |
| `rust/src/input.rs` | Win32 Unicode `SendInput` |
| `rust/src/transcript.rs` | focus-checked fixed-rate text queue |
| `rust/src/parakeet_native.rs` | dynamic loading and C API calls for `parakeet.dll` |
| `rust/src/config.rs` | native-only settings and runtime path validation |
| `rust/src/gui.rs` | Slint settings window and non-blocking UI callbacks |
| `rust/src/tray.rs` | notification-area icon and tray menu |
| `rust/src/overlay.rs` | Slint click-through recording visualizer |
| `rust/src/screenshots.rs` | real Slint PNG UI screenshots |

## Runtime Invariants

- No CPU fallback is allowed.
- Runtime files live under `external/parakeet-runtime/parakeet-windows-cuda` unless config overrides them.
- The model path must point to a GGUF Parakeet model.
- Only a transcript for the current released recording may be typed.
- Typing aborts if the foreground window changes.

## Testing Strategy

Rust tests should focus on platform-neutral logic: config, resampling, and transcript queue cancellation. Native CUDA behavior is verified through `transcribe-file` with `tests/fixtures/parakeet-smoke.wav`. Windows desktop behavior must be manually exercised with `run`.

## UI Iteration Loop

When changing UI layout, use deterministic screenshots so agents can inspect the result:

```powershell
cargo run -p uvox -- ui-screenshot --surface settings --section audio --output artifacts\ui-settings-audio.png
cargo run -p uvox -- ui-screenshot --surface settings --section model --output artifacts\ui-settings-model.png
cargo run -p uvox -- ui-screenshot --surface settings --section logging --output artifacts\ui-settings-logging.png
cargo run -p uvox -- ui-screenshot --surface overlay --output artifacts\ui-overlay.png
```

Inspect the PNGs, adjust layout/colors/sizing, and repeat until the settings window and overlay are readable and not cramped. Then run `cargo check -p uvox` and `cargo test -p uvox`.
