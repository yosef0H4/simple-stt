# Simple STT architecture overhaul — source-only completion report

## Status

The repository has been redesigned around a thin AutoHotkey v2 desktop shell, a persistent lightweight Rust capture service, and a disposable Rust inference worker. The source tree, documentation, IPC proof of concept, and static architectural audit are complete.

This execution environment does not provide Cargo, Rust, AutoHotkey v2, PowerShell, a Windows desktop session, a microphone, the Parakeet runtime, a GGUF model, or an NVIDIA GPU. Therefore no local Rust compilation, AutoHotkey execution, microphone recording, model inference, RAM measurement, or VRAM measurement was performed here. The Windows scripts and smoke tests are included for target-machine validation.

## 1. Architecture summary

```text
ahk/simple-stt.ahk or packaged simple-stt.exe
    AutoHotkey v2 desktop shell
    ├── tray, settings GUI, dynamic hold-to-record hotkeys
    ├── Caps Lock tap preservation and AltGr-aware modifier handling
    ├── target-window tracking and Unicode SendText() typing
    ├── exact-PID capture-service supervision and startup shortcut
    └── asynchronous one-shot simple-stt-ctl.exe helper launches

simple-stt-ctl.exe
    disposable local control helper
    ├── reads token-authenticated loopback discovery state
    ├── exchanges versioned JSON-line messages with simple-stt-capture.exe
    └── publishes escaped UTF-8 tab response files atomically for AHK polling

simple-stt-capture.exe
    persistent lightweight Rust service
    ├── CPAL microphone capture, PCM buffering, gain, downmix and 16 kHz resampling
    ├── RMS visualizer level and fast Rust overlay
    ├── structured component logs and loopback-only IPC server
    ├── HTTPS model downloads to unique partial files with atomic rename
    └── lazy simple-stt-infer.exe lifecycle supervision with exact-PID termination fallback

simple-stt-infer.exe
    disposable Rust child worker
    ├── the only active process that dynamically loads parakeet.dll
    ├── lazy GGUF model loading and framed PCM inference over child stdin/stdout
    ├── warm reuse before the configured idle timeout
    └── graceful shutdown, flush, free attempt, then complete process exit
```

No raw PCM travels through AutoHotkey. Process exit, rather than DLL unload, is the memory-reclamation guarantee.

## 2. File changes

### Added or replaced active implementation

- `ahk/simple-stt.ahk`
- `ahk/lib/{Config,Hotkeys,IpcClient,Logging,ProcessSupervisor,SettingsGui,TabProtocol,Tray,Typist,Utils}.ahk`
- `ahk/tests/{hotkeys-manual,ipc-smoke,typing-smoke}.ahk`
- `src/bin/{simple_stt_capture,simple_stt_infer,simple-stt-ctl}.rs`
- `src/bin/simple_stt_mock_infer.rs` for deterministic integration tests only
- `src/capture/{audio,inference_supervisor,ipc_server,overlay,overlay_windows,process,state}.rs`
- `src/infer/{parakeet_native,protocol}.rs`
- `src/common/{line_codec,shell_protocol}.rs`
- `src/{config,logging,models,lib}.rs`
- `tests/worker_lifecycle.rs`
- `fixtures/parakeet-smoke.wav`
- `tools/ipc-poc/{README,mock_service,test_poc}.py`
- `scripts/{build-release,check-prereqs,ipc-poc,memory-cleanup-validation,package-release,run-dev,test-static}.ps1`
- `scripts/verify-static.py`
- `docs/{ahk-v2-research,current-behavior-inventory,ipc-decision,architecture,configuration,packaging,memory-cleanup-validation,testing,change-manifest}.md`

### Archived instead of deleted

- Original monolith sources are retained under `legacy/monolith-root/`.
- The duplicate Rust package is retained under `legacy/duplicate-rust-package/`.
- Obsolete monolith-era tools are retained under `legacy/obsolete-root/`.

### Removed from the active Cargo graph

- Slint settings UI
- Rust tray ownership
- Rust global keyboard hooks
- Rust final text injection
- In-process Parakeet loading and model unload cleanup

The stale pre-overhaul lockfile was removed. `Cargo.lock` is intentionally **not** ignored; generate and commit a fresh lockfile from the new dependency graph during the first Windows release build.

## 3. AutoHotkey v2 research summary

`docs/ahk-v2-research.md` documents the official v2 APIs and caveats used by the shell: mandatory v2 directives, `Hotkey()`, `HotIf()` / `#HotIf`, custom combinations, down/up variants, `KeyWait()`, `SendText()`, `SendInput()`, `SendMode()`, `SetKeyDelay()`, tray APIs, menu objects, GUI controls and events, timers, process APIs, exit/error hooks, messages, files and pipes, `DllCall()`, `Persistent()`, `FileCreateShortcut()`, Ahk2Exe directives, startup registration, v1-to-v2 differences, custom-combination wildcard behavior, AltGr, and backtick escape syntax.

Chosen shell rules:

- Every AHK entry point begins with `#Requires AutoHotkey v2.0` and `#SingleInstance Force`.
- Hotkey callbacks only launch bounded helper jobs; `SetTimer()` polls their response files.
- Caps Lock custom combinations do not add an invalid wildcard prefix because combinations already wildcard-match extra modifiers.
- Unicode text is typed with chunked `SendText()` after verifying the target foreground window before every chunk.
- Clipboard typing is not the default and is not implemented in the active typist.

## 4. IPC decision and proof of concept

`docs/ipc-decision.md` compares Windows named pipes, `WM_COPYDATA`, and a one-shot helper. The selected AHK-to-capture transport is `simple-stt-ctl.exe` plus token-authenticated loopback TCP:

- lower AHK Win32 API surface than direct named-pipe handling;
- no synchronous `WM_COPYDATA` callback path;
- no long socket or pipe read on the AHK UI thread;
- versioned JSON envelopes and structured errors in Rust;
- per-capture-launch random token and local-only `127.0.0.1` binding;
- reconnect after service restart through atomically replaced discovery state;
- escaped UTF-8 tab response files for a parser-light AHK client.

Rust capture-to-worker IPC uses framed binary anonymous child stdin/stdout. Human-readable logs never share worker stdout.

Executed successfully in this environment:

```text
python3 tools/ipc-poc/test_poc.py
IPC POC PASSED
 - authenticated loopback PING/PONG
 - START_RECORDING / STOP_RECORDING
 - asynchronous polled Unicode TRANSCRIPT
 - state-file reconnect after simulated service restart
```

## 5. Windows build instructions

From an x64 Windows PowerShell prompt with Rust installed:

```powershell
.\scripts\check-prereqs.ps1 -RequireRuntime
.\scripts\build-release.ps1
cargo test --all-targets
```

The release script deliberately builds only:

```text
simple-stt-capture.exe
simple-stt-infer.exe
simple-stt-ctl.exe
```

The test-only mock worker is excluded from the release build.

## 6. Development run instructions

Install AutoHotkey v2 for script-mode development, provide the Parakeet runtime and GGUF model as documented, then run:

```powershell
.\scripts\run-dev.ps1
```

To launch after building separately:

```powershell
.\scripts\run-dev.ps1 -SkipBuild
```

The shell launches and supervises the persistent capture service. The capture service launches the inference worker lazily only when a transcript requires it.

## 7. Packaging instructions

Build the current Windows distribution with:

```powershell
build-distribution.cmd
```

The underlying implementation lives in `scripts\build-distribution.ps1`; `scripts\package-release.ps1` remains a compatibility wrapper.

The staged portable runtime includes:

```text
simple-stt.exe
simple-stt-capture.exe
simple-stt-infer.exe
simple-stt-ctl.exe
fixtures\parakeet-smoke.wav
external\parakeet-runtime\parakeet-windows-cuda\...
```

Runtime DLLs and GGUF models are intentionally supplied separately under documented install-relative directories. Start-with-Windows creates a shortcut targeting the AHK shell or compiled `simple-stt.exe`, never the retired monolithic Rust executable.

## 8. Tests and results

Executed source-only checks:

```text
python3 scripts/verify-static.py
STATIC VERIFY PASSED
 - release lockfile hygiene is explicit: Cargo.lock is not ignored
 - shell entry and AHK smoke scripts are v2-only
 - active Cargo graph has split binaries and no Slint frontend dependency
 - Parakeet DLL/model loading is isolated to disposable simple-stt-infer with exact-PID forced-exit fallback
 - AHK owns tray, GUI, hotkeys, Caps Lock behavior, and foreground-safe Unicode typing
 - AHK helper IPC is asynchronous, token-rotated, sequenced, and reconnectable
 - control IPC is loopback-only and versioned; raw PCM stays on framed child pipes
 - schema-v2 migration, install-relative paths, atomic writes, HTTPS partial downloads, and diagnostics are present
 - mock-worker lifecycle integration sources cover reuse, idle exit, model switch, crash recovery, and forced shutdown
 - retired monolith is archived under legacy instead of deleted
```

Also executed:

```text
python3 tools/ipc-poc/test_poc.py
python3 -m py_compile scripts/verify-static.py tools/ipc-poc/mock_service.py tools/ipc-poc/test_poc.py
```

Windows target-machine checks still required:

```powershell
cargo test --all-targets
.\scripts\test-static.ps1
.\scripts\ipc-poc.ps1
& "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe" .\ahk\tests\hotkeys-manual.ahk
& "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe" .\ahk\tests\ipc-smoke.ahk
& "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe" .\ahk\tests\typing-smoke.ahk
```

## 9. RAM and VRAM cleanup validation

`docs/memory-cleanup-validation.md` and `scripts/memory-cleanup-validation.ps1` implement the repeatable Windows diagnostic flow. The script records shell and capture baselines, launches inference, exercises a model test and transcription fixture, records worker RAM and optional `nvidia-smi` output, unloads the model, confirms the worker exits, verifies its process RAM disappears, checks that capture remains lightweight, and captures post-exit VRAM evidence.

Observed RAM and VRAM numbers are **not available from this environment**. The architectural guarantee is implemented: the model-owning process exits completely after idle unload or explicit unload, with an exact-child-PID forced-termination fallback if graceful worker shutdown cannot complete.

Run on the target GPU machine:

```powershell
.\scripts\memory-cleanup-validation.ps1
```

Review the generated evidence directory under `artifacts/` and copy observed values into `docs/memory-cleanup-validation.md` before release sign-off.

## 10. Remaining known limitations

- The overhaul has not been compiled or run on Windows in this environment. Complete the documented target-machine build, AHK manual smoke tests, microphone tests, Parakeet model test, and diagnostics before shipping.
- Authoritative model checksums were not found in the original repository. Downloads are HTTPS-only, filename-validated, written to unique partial files, and atomically renamed; add checksum enforcement when authoritative values are available.
- Runtime DLLs and GGUF files are not bundled in source control.
- AutoHotkey text injection remains subject to Windows privilege boundaries: a normal shell cannot type into a higher-integrity target. The foreground-window safety check prevents typing into the wrong active window but does not bypass UAC.
- Rust log writers prefix every emitted line with component and PID; session IDs remain attached to recording, inference, transcript-length, and typing events where applicable.
- Rapid repeated dictations are source-hardened with per-session typing targets, deferred stop sequencing, pending transcript tracking, and overlay settlement that preserves newer recording/transcribing visual state when an older result completes.
- The retained legacy tree is intentionally large so behavior remains inspectable during staged Windows validation. Remove it only after equivalent behavior is confirmed.
