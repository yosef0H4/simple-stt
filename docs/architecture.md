# Simple STT architecture after overhaul

## Process boundaries

```text
simple-stt.cmd
    launches bundled AutoHotkey64.exe with simple-stt.ahk
    thin AutoHotkey v2 desktop shell
    ├── tray icon and stateful menu
    ├── runtime hold/toggle recording hotkeys and Caps Lock tap behavior
    ├── foreground target capture and Unicode transcript typing
    ├── start-with-Windows shortcut
    ├── user-facing notices and shell log
    └── exact-PID supervision of simple-stt-capture.exe

simple-stt-ctl.exe
    disposable one-shot control helper
    ├── reads discovery state file
    ├── performs protocol/token handshake
    ├── sends one command to capture service over loopback
    ├── translates JSON response to escaped UTF-8 tab records
    └── exits

simple-stt-capture.exe
    persistent lightweight Rust service
    ├── CPAL microphone capture
    ├── gain, mono downmix, linear 16 kHz resampling
    ├── PCM buffering only while recording
    ├── fast overlay and RMS level updates
    ├── local control server and structured events
    ├── model downloads and tests off the control thread
    ├── optional asynchronous AI transcript cleanup and in-memory history
    ├── optional bounded, in-memory screen context capture
    └── lazy supervision of simple-stt-infer.exe

simple-stt-settings.exe
    disposable authenticated loopback settings server
    ├── serves bundled vanilla HTML/CSS/JavaScript to the default browser
    ├── edits canonical nested config.json with explicit Save
    ├── streams capture/model events to the browser
    ├── starts model refresh, download, selection, and test workflows
    ├── configures/tests AI cleanup and owns short-lived OAuth callbacks
    ├── stores provider credentials only in the operating-system vault
    └── exits on Close or idle timeout; repeated opens reuse its session

simple-stt-infer.exe
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
| Linux global shortcuts | `simple-stt-linux` | Automatic backend selection uses GlobalShortcuts on Wayland, native X11 passive grabs on X11, and documented compositor commands when no Wayland portal exists. KDE Plasma on Fedora Wayland is the only real-hardware-tested Linux environment; other desktops remain experimental. |
| Settings UI | `simple-stt-settings` | Cross-platform browser UI; no webview, Node, or frontend framework. |
| User hotkeys | AHK shell | Runtime `Hotkey()` bindings; CapsLock custom combination path. |
| Final typing | AHK shell | Variable-paced per-character `SendText()`; target HWND checked before every character. |
| Service PID supervision | AHK shell | PID from `Run()`; graceful request then `ProcessWaitClose()` and exact-PID `ProcessClose()` fallback. |
| Audio capture | capture service | CPAL stream stays warm while shell runs. |
| Microphone preference | capture service | Empty preference follows the system default; a stable device ID is pinned and falls back temporarily. On Windows, Core Audio endpoint notifications trigger bounded readiness retries. The device is also resolved at recording start as a safety net, and switching never interrupts an active dictation. |
| PCM buffer | capture service | Allocated/grown only for active recording. |
| Rapid recording visualizer | capture service | Rust Win32 overlay. |
| Parakeet DLL and model | infer worker | Isolated; capture service cannot import loader. |
| Model idle cleanup | capture service + infer worker | request graceful worker shutdown, then process exit; exact-PID force kill only after grace period. |
| AI transcript cleanup | capture service | Optional and disabled by default. Runs after STT and before AHK transforms/delivery. Failures and timeouts deliver the original transcript. It never loads speech models. |
| Screen context | capture service | Separately opted in. Windows uses the recording target HWND, X11 captures the active window, and Wayland uses a compositor-owned portal prompt. Images remain in memory and are bounded before upload. |
| AI credentials | OS vault | Windows Credential Manager or the Linux desktop secret service. `SIMPLE_STT_AI_API_KEY` is an explicit process-environment override; secrets never enter `config.json` or logs. |
| Canonical config | schema-v7 JSON | Nested portable JSON; browser UI is only an editor. |
| Component logs | each component | Shell, capture, and infer logs are separate. |

## Dictation sequence

```text
AHK hotkey down
  capture foreground HWND
  assign session id
  asynchronously launch: simple-stt-ctl start-recording

capture service
  enter Recording overlay state
  append future 16 kHz PCM frames to active session buffer

AHK hotkey up
  asynchronously launch: simple-stt-ctl stop-recording

capture service
  stop buffer
  reject clips shorter than 100 ms
  set Transcribing overlay state
  lazily launch or reuse simple-stt-infer
  send framed PCM request on child stdin

simple-stt-infer
  lazy-load parakeet.dll and selected GGUF if needed
  transcribe PCM
  return framed Unicode transcript on stdout

capture service
  if AI cleanup is enabled, send transcript and optional screen context to the selected provider
  keep at most five raw/cleaned pairs in process memory
  queue cleaned transcript event, or original transcript on any cleanup failure

AHK poll timer
  launch simple-stt-ctl poll-events
  receive transcript event
  queue transcript for variable-paced per-character SendText typing
  verify same HWND before every chunk
```

A later dictation can be recorded while an earlier inference or cleanup request is completing. Shell target windows are tracked by session and transcript typing is queued, avoiding loss of rapid repeated dictations.

On Windows, every app hotkey may be set to `None`. The AI-cleanup toggle defaults to `None`; when assigned, it atomically flips `cleanup.enabled`, requests capture-service reload, and affects the next dictation. On Wayland the equivalent action is compositor-assigned through the GlobalShortcuts portal.

AI cleanup is stateless at the provider boundary: each request contains only the current transcript, the configured system instructions, and at most one current screenshot. Transcript and screen text are explicitly delimited as untrusted content. The five-item Settings history is memory-only and disappears with the capture process.

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

`simple-stt-capture.exe` never links or imports the active native loader module. Static verification checks that `libloading` appears only in `src/infer/parakeet_native.rs`. Capture control handlers read the child PID from an atomic tracker rather than waiting on the worker mutex, preserving responsiveness even when inference is blocked.

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

Relative runtime and model directories resolve against the runtime root. Checkout builds under `target\debug` or `target\release` walk back to the repository root. Packaged shortcuts target `simple-stt.cmd`, which launches `runtime\AutoHotkey64.exe` with `runtime\simple-stt.ahk`; the shell then resolves relative paths against the installed runtime directory. The bundled smoke fixture is `fixtures\parakeet-smoke.wav`.

## Helper subprocess completion

The AHK shell does not trust helper PID disappearance alone. `simple-stt-ctl` publishes its response file atomically; the shell timer accepts that file as completion, applies a bounded helper timeout, and terminates only the tracked helper PID on timeout. Readiness probes are de-duplicated.

## Structured log prefix

Every Rust log writer prefixes each emitted line with `component=<capture|infer> pid=<pid>`. Tracing supplies timestamps and per-event fields such as `session_id`; the AHK shell log uses the same component/PID/session convention. Transcript contents remain disabled unless `log_transcripts` is explicitly enabled; otherwise only character counts are recorded. Release builds force minimal logging, and each component log is truncated after seven days or 2 MiB so application logs remain bounded without accumulating rotated files.
