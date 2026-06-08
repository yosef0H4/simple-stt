# Skill: run Uvox smoke tests

Source-only checks:

```powershell
.\scripts\test-static.ps1
```

Windows release checks:

```powershell
.\scripts\check-prereqs.ps1 -RequireRuntime
.\scripts\build-release.ps1
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5
.\scripts\run-dev.ps1 -SkipBuild
```

Use the AHK manual scripts under `ahk\tests\` for hotkeys, Unicode typing, and IPC. Do not add a CPU fallback when CUDA or the Parakeet runtime is missing.
