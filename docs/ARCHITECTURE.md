# Uvox architecture after overhaul

## Process boundaries

```text
uvox.ahk or compiled uvox-shell.exe
    thin AutoHotkey v2 desktop shell
    ├── tray icon and stateful menu
    ├── settings GUI and hotkey recorder
    ├── runtime hold-to-record hotkeys and Caps Lock tap behavior
    ├── foreground target capture and Unicode transcript typing
    ├── start-with-Windows shortcut
    ├── user-facing notices and shell log
    └── exact-PID supervision of uvox-capture.exe

uvoxctl.exe
    disposable one-shot control helper
    ├── reads discovery state file
    ├── performs protocol/token handshake
    ├── sends one command to capture service over loopback
    ├── translates JSON response to escaped UTF-8 tab records
    └── exits

uvox-capture.exe
    persistent lightweight Rust service
    ├── CPAL microphone capture
    ├── gain, mono downmix, linear 16 kHz resampling
    ├── PCM buffering only while recording
    ├── fast overlay and RMS level updates
    ├── local control server and structured events
    ├── model downloads and tests off the control thread
    └── lazy supervision of uvox-infer.exe

uvox-infer.exe
    disposable Rust inference worker
    ├── only active component allowed to load parakeet.dll
    ├── only active component allowed to load GGUF models
    ├── framed PCM/WAV-test protocol on stdin/stdout
    ├── warm reuse during configured idle window
    └── graceful shutdown plus process exit cleanup guarantee
```

## Ownership matrix

| Feature | Owner | Notes |
| --- | --- | --- |
| Tray icon/menu | AHK shell | `A_TrayMenu`, menu object APIs, `TraySetIcon()`. |
| Settings GUI | AHK shell | No Slint in active build. |
| User hotkeys | AHK shell | Runtime `Hotkey()` bindings; CapsLock custom combination path. |
| Final typing | AHK shell | `SendText()` chunks; target HWND checked before every chunk. |
| Service PID supervision | AHK shell | PID from `Run()`; graceful request then `ProcessWaitClose()` and exact-PID `ProcessClose()` fallback. |
| Audio capture | capture service | CPAL stream stays warm while shell runs. |
| PCM buffer | capture service | Allocated/grown only for active recording. |
| Rapid recording visualizer | capture service | Rust Win32 overlay. |
| Parakeet DLL and model | infer worker | Isolated; capture service cannot import loader. |
| Model idle cleanup | capture service + infer worker | request graceful worker shutdown, then process exit; exact-PID force kill only after grace period. |
| Canonical config | schema-v2 JSON | Read/write through Rust config module; shell edits via `uvoxctl`. |
| Component logs | each component | Shell, capture, and infer logs are separate. |

## Dictation sequence

```text
AHK hotkey down
  capture foreground HWND
  assign session id
  asynchronously launch: uvoxctl start-recording

capture service
  enter Recording overlay state
  append future 16 kHz PCM frames to active session buffer

AHK hotkey up
  asynchronously launch: uvoxctl stop-recording

capture service
  stop buffer
  reject clips shorter than 100 ms
  set Transcribing overlay state
  lazily launch or reuse uvox-infer
  send framed PCM request on child stdin

uvox-infer
  lazy-load parakeet.dll and selected GGUF if needed
  transcribe PCM
  return framed Unicode transcript on stdout

capture service
  queue transcript event

AHK poll timer
  launch uvoxctl poll-events
  receive transcript event
  queue transcript for paced SendText typing
  verify same HWND before every chunk
```

A later dictation can be recorded while an earlier inference request is completing. Shell target windows are tracked by session and transcript typing is queued, avoiding loss of rapid repeated dictations.

## Worker cleanup sequence

```text
idle timeout reached OR Unload Speech Model OR model/runtime/timeout change OR capture shutdown
  capture sends framed Shutdown
  worker drops model context where possible
  worker flushes file logger as process exits
  capture polls child exit until worker_shutdown_grace_ms
  if the normal path is blocked behind inference: independent atomic PID tracker remains readable
  if still running: capture opens and terminates only that exact child PID
  operating system reclaims worker RAM and VRAM allocations with process exit
```

`uvox-capture.exe` never links or imports the active native loader module. Static verification checks that `libloading` appears only in `src/infer/parakeet_native.rs`. Capture control handlers read the child PID from an atomic tracker rather than waiting on the worker mutex, preserving responsiveness even when inference is blocked.

## Overlay state model

The capture service uses explicit primary states:

```text
Hidden
Recording
Transcribing
```

Transient notices are layered on top, including:

```text
Loading speech model…
No speech detected
Recording too short
Speech engine failed — see log
Audio service error — see log
```

Routine unload stays in logs unless diagnostic overlay is enabled.

## Install-relative runtime paths

Relative runtime and model directories resolve against the runtime root. Checkout builds under `target\debug` or `target\release` walk back to the repository root. Packaged binaries staged beside `uvox-shell.exe` resolve relative paths against that installed directory. The bundled smoke fixture is `fixtures\parakeet-smoke.wav`.

## Helper subprocess completion

The AHK shell does not trust helper PID disappearance alone. `uvoxctl` publishes its response file atomically; the shell timer accepts that file as completion, applies a bounded helper timeout, and terminates only the tracked helper PID on timeout. Readiness probes are de-duplicated.

## Structured log prefix

Every Rust log writer prefixes each emitted line with `component=<capture|infer> pid=<pid>`. Tracing supplies timestamps and per-event fields such as `session_id`; the AHK shell log uses the same component/PID/session convention. Transcript contents remain disabled unless `log_transcripts` is explicitly enabled.
