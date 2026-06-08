# Contributing

Read `AGENTS.md`, `docs/architecture.md`, `docs/configuration.md`, and `docs/testing.md` first.

Keep the ownership boundaries strict:

```text
AHK: tray, GUI, hotkeys, Caps Lock, startup, notices, target-window safety, final typing, capture supervision
capture service: CPAL, PCM buffers, overlay, shell IPC, downloads, disposable-worker lifecycle
infer worker: parakeet.dll, GGUF model, local speech inference, complete process exit
```

Run source checks:

```powershell
.\scripts\test-static.ps1
```

Run Windows Rust and desktop checks before release:

```powershell
cargo test --all-targets
.\scripts\run-dev.ps1 -SkipBuild
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5
```

Do not add clipboard typing as the default, CPU fallback, browser desktop frameworks, random anti-detection timing, or hidden behavior intended to bypass target-application restrictions.
