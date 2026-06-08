# Uvox

Uvox is a Windows-only local dictation application redesigned around a thin AutoHotkey v2 desktop shell, a persistent lightweight Rust audio service, and a disposable Rust Parakeet inference worker.

```text
uvox.ahk or uvox-shell.exe
  └── uvox-capture.exe      persistent CPAL capture + fast overlay
        └── uvox-infer.exe  disposable Parakeet DLL/model process

uvoxctl.exe                 disposable AHK control helper
```

The key memory-cleanup guarantee is process exit: `uvox-infer.exe` is the only active component allowed to load `parakeet.dll` or a GGUF model. Repeated dictations reuse a warm worker until the configured idle timeout. Cleanup terminates that worker so Windows can reclaim its process RAM and VRAM allocations.

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
.\scripts\check-prereqs.ps1 -RequireRuntime
.\scripts\build-release.ps1
.\scripts\run-dev.ps1 -SkipBuild
```

## Shell-owned behavior

The AutoHotkey v2 shell owns the tray icon/menu, settings GUI, runtime hold-to-record hotkeys, hotkey recorder, Caps Lock tap behavior, start-with-Windows shortcut, foreground target tracking, Unicode `SendText()` chunking, log opening, user notices, and exact-PID capture-service supervision.

The shell does not parse JSON, move PCM, read a long pipe, load the model, or perform a blocking socket operation from a callback. It starts disposable `uvoxctl.exe` commands and polls completion with `SetTimer()`.

## Rust-owned behavior

`uvox-capture.exe` keeps CPAL audio warm, applies gain, downmixes, linearly resamples to 16 kHz mono PCM16, computes overlay levels, buffers only active recordings, owns the rapid Win32 overlay, publishes local control events, and supervises the worker.

`uvox-infer.exe` dynamically loads Parakeet, loads the selected GGUF lazily, handles framed PCM/WAV requests, returns transcripts, and exits after graceful shutdown or its idle backstop.

## Source-only checks

```powershell
.\scripts\test-static.ps1
```

The overhaul was authored without building in the editing container. The dependency-free IPC POC and static boundary checks were executed there; Windows compilation, AHK smoke tests, audio inference, and RAM/VRAM measurements remain explicit target-machine validation steps.

## Legacy source

The retired monolith is no longer checked into the working tree. Use git history if you need to inspect the old structure.

## License

GPL-2.0-only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.


## Install-relative paths

Relative runtime/model directories resolve from the checkout root for Cargo `target\debug` / `target\release` binaries and from the executable directory for packaged binaries. `scripts\package-release.ps1` stages `fixtures\parakeet-smoke.wav` for model testing.
