# Memory-cleanup validation

## Why the architecture changed

The retired monolith called a native unload path inside the resident Rust process. That may release model state and VRAM but cannot guarantee that every allocator page is returned to Windows. The new cleanup guarantee is worker process exit.

Only `uvox-infer.exe` imports `libloading` and loads `parakeet.dll`. `uvox-capture.exe` remains resident but does not load the model DLL. After idle timeout or manual unload, capture asks the worker to shut down, waits for its PID to exit, and force-terminates only that child PID if it hangs past `worker_shutdown_grace_ms`. An independent atomic PID tracker keeps that fallback available even when a transcript read is blocked behind the supervisor mutex.

## Repeatable Windows diagnostic

Run after building release binaries and making the runtime/model available:

```powershell
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5
```

Optional parameters:

```powershell
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5 -ShellPid <pid> -KeepProcesses
```

The script:

1. chooses an isolated temporary schema-v2 config;
2. records optional shell RAM;
3. launches `uvox-capture.exe` with an exact PID and random token;
4. records baseline capture RAM;
5. requests a model smoke test through `uvoxctl.exe`;
6. polls service state until the infer-worker PID is visible;
7. records infer RAM;
8. invokes `nvidia-smi` when available;
9. records post-transcription process data;
10. requests `unload-model`;
11. confirms the infer-worker PID exits;
12. verifies the worker RAM record disappears with process exit;
13. records capture RAM after cleanup;
14. records VRAM snapshot after exit;
15. saves JSON evidence under `artifacts\memory-cleanup-validation-<timestamp>.json`.

The smoke test uses `fixtures\parakeet-smoke.wav`; it exercises the same disposable worker model-load boundary without requiring microphone interaction. A manual dictation pass should be run as an additional product check.

## Acceptance criteria

```text
infer worker launches lazily
infer worker has measurable RAM while model is loaded
infer worker PID exits after unload or idle period
Get-Process no longer returns worker RAM after exit
capture service remains alive
capture service WorkingSet after cleanup remains near its lightweight baseline
nvidia-smi shows model VRAM released after worker exit, when available
```

“Near baseline” is intentionally observational rather than a hardcoded byte limit because CPAL, Windows audio buffers, logging, and allocator behavior vary across systems. Any large sustained increase in capture RAM after several cycles is a bug to investigate.

## Observed results in the editing environment

Not measured. The source-only Linux editing environment does not have Windows processes, Cargo-built executables, CUDA, a Parakeet runtime, or `nvidia-smi`. It cannot truthfully produce RAM or VRAM numbers.

What was verified statically:

```text
active capture modules do not import ParakeetNative or libloading
active crate imports libloading only in src/infer/parakeet_native.rs
capture supervisor sends graceful Shutdown before exact-child-PID fallback
mock-worker integration source exercises the blocked-read force path
worker logs that process exit is the memory cleanup guarantee
```

The Windows diagnostic script is included so observed machine-specific figures can be captured rather than invented.
