# Current behavior inventory before the overhaul

This inventory was recorded from the archived monolithic source. The working tree now contains only the split architecture; use git history if you need to recover the removed monolith files.

## Old executable shape

The root package built one resident Rust application. `src/main.rs` owned CLI dispatch, the tray utility, Slint settings, low-level keyboard hook, foreground-window checks, typed-text injection, CPAL microphone capture, overlay rendering, native Parakeet FFI, and idle DLL unload. A duplicate Rust tree also existed under `rust/`; both trees are now removed from the working tree to keep the active build unambiguous.

Old CLI commands:

```text
run
settings
config-show
config-reset
list-inputs
transcribe-file --audio <wav>
ui-screenshot --surface settings|overlay|overlay-desktop --output <png>
tooltip-bench --iterations <n>     (hidden)
```

## Old tray commands

The Rust tray menu exposed:

```text
Open Settings
Disable / Enable Hotkey
Reload Config
Open Log
Unload Model
Test Model
Exit
```

The new shell preserves these behaviors and adds `Restart Audio Service`; labels are adjusted to the requested wording.

## Old persisted config and defaults

The old JSON was stored at the platform config directory as `simple-stt/config.json`, overrideable through `SIMPLE_STT_CONFIG`. It had no explicit schema version.

| Old field | Old default | Notes |
| --- | --- | --- |
| `idle_timeout_secs` | `180` | In-process Parakeet-context unload timeout. |
| `typing_interval_ms` | `20` | Delay between typed chunks. |
| `typing_chunk_chars` | `3` | Unicode chunk size. |
| `audio_gain` | `1.0` | Applied before PCM conversion. |
| `audio_device_contains` | `""` | Empty chooses default microphone; otherwise case-insensitive substring matching. |
| `parakeet_runtime_dir` | `external\\parakeet-runtime\\parakeet-windows-cuda` | Runtime root. |
| `parakeet_model_path` | `...\\models\\tdt_ctc-110m-f16.gguf` | Full selected model path. |
| `start_with_windows` | `false` | Old Rust startup registration. |
| `hotkey_enabled` | `true` | Runtime enable/disable. |
| `record_hotkey` | `capslock+s` | Configurable hold chord. |
| `capslock_always_off` | `false` | Optional forced-off behavior. |
| `log_level` | `minimal` | `minimal`, `normal`, `debug`, or `extreme`. |

Schema-v2 migration maps this to `idle_worker_timeout_secs`, `model_dir`, `selected_model_filename`, and `capslock_behavior`. A backup is written before migration.

## Old hotkey semantics

The old Rust low-level keyboard hook parsed one non-modifier key plus modifiers. Supported modifier classes included CapsLock, Ctrl, Shift, Win, left Alt, and right Alt. It filtered injected low-level events, recognized key-down and key-up, and had special Caps Lock handling. Default behavior was hold `CapsLock+S` to record and release the final key to stop. The old README text had drifted and sometimes described holding CapsLock alone; source behavior is authoritative.

The overhaul moves all user-facing binding and tap behavior into AHK v2. Manual scripts cover CapsLock tap, CapsLock+S hold/release, generic and left/right modifiers, AltGr, reassignment, enable/disable, and generated typing.

## Old audio behavior

The useful audio path is preserved in `src/capture/audio.rs`:

- CPAL default host and default microphone when no match string is configured.
- Optional case-insensitive device-name substring selection.
- Default input stream config from CPAL.
- Accepted device formats: `f32`, `i16`, and `u16`.
- Downmix all configured channels to mono.
- Apply configured gain.
- Linear resampling to `16_000` Hz.
- Emit `320` PCM16 samples per frame, which is `20 ms` at 16 kHz.
- Compute a smoothed/expanded RMS-derived visualizer level from each emitted frame.
- Buffer frames only during active recording in the new service.

The old audio channel capacity was `4096` frames; the new capture service keeps that value.

## Old overlay behavior

The old Rust overlay rendered a rapidly updating Win32 tooltip/overlay driven by an atomic latest-level value. This is intentionally kept in Rust because it updates far more frequently than the AHK shell should. The new wrapper introduces explicit primary states and transient notices instead of arbitrary UI strings.

## Old typing safety behavior

The old Rust typist remembered the foreground HWND at recording start. Before each typed chunk it checked that the current foreground HWND still matched. On mismatch, it rejected typing. It inserted text through a normal Windows text sink at configurable chunk size and delay.

The shell now owns this behavior and sends literal Unicode with AHK `SendText()` without clipboard use. It queues rapid transcripts by session and repeats the foreground check before every chunk.

## Old model behavior

The monolith loaded `parakeet.dll` and the selected GGUF model directly in its own resident process. The idle watcher requested `UnloadParakeetContext` every five seconds once the configured timeout had elapsed. This could free native context and VRAM while leaving allocator/process RAM resident.

The new guarantee is stronger: only `simple-stt-infer.exe` loads Parakeet. Cleanup is verified by worker process exit rather than trusting DLL unload.

The old app had:

- an approved model catalog;
- HTTPS downloads from a Hugging Face repository;
- `.partial` writes followed by rename;
- WAV smoke-test transcription;
- model selection in Slint settings.

The new service preserves those capabilities. Checksums remain a known limitation because authoritative per-file checksums were not bundled in the repository.

## Old startup and logs

`src/startup.rs` modified a per-user Windows Run registry value. The new shell uses an `A_Startup` shortcut that launches `simple-stt.ahk` in development or `simple-stt-shell.exe` when compiled.

The old logger wrote one `latest.log`. The new architecture writes append-style component logs:

```text
simple-stt-shell.log
simple-stt-capture.log
simple-stt-infer.log
```

Transcripts are not logged by default.

## Old utilities and tests

Archived utilities:

- deterministic settings/overlay screenshot rendering;
- hidden tooltip benchmark;
- WAV transcription smoke test;
- `tests/idle_unload_integration.rs`, which observed in-process unload behavior;
- `tests/fixtures/parakeet-smoke.wav`.

The original fixture was preserved and copied to active install-relative `fixtures/parakeet-smoke.wav` so packaged model tests do not depend on a source checkout. The obsolete idle-unload integration is archived and replaced by process-exit validation scripts.

## Missing runtime files and environmental fixtures

The repository intentionally does not bundle the large native Parakeet runtime or GGUF models. The following are expected externally:

```text
external\\parakeet-runtime\\parakeet-windows-cuda\\bin\\parakeet.dll
external\\parakeet-runtime\\parakeet-windows-cuda\\models\\<approved model>.gguf
```

A Windows target, AutoHotkey v2 for development, Cargo/Rust, CPAL-compatible microphone device, and CUDA-capable Parakeet runtime are required for end-to-end validation.
