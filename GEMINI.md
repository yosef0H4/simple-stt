# Simple STT agent entry point

Read `AGENTS.md` first. The active architecture is:

```text
AutoHotkey v2 shell -> persistent simple-stt-capture.exe -> disposable simple-stt-infer.exe
                     disposable simple-stt-ctl.exe control helper
```

Do not reintroduce Rust tray code, Rust settings UI, global Rust hooks, Rust transcript injection, or Parakeet loading inside the resident capture service. Use `legacy/` only as migration reference.

Before editing AHK, use the official v2 documentation summarized in `docs/ahk-v2-research.md`. Do not use AutoHotkey v1 command syntax.

Useful source-only checks:

```powershell
.\scripts\test-static.ps1
```

Windows validation:

```powershell
.\scripts\check-prereqs.ps1 -RequireRuntime
.\scripts\build-release.ps1
.\scripts\run-dev.ps1 -SkipBuild
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5
```
