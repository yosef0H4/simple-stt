# Agent notes

Read `docs/architecture.md`, `docs/ahk-v2-research.md`, and `docs/current-behavior-inventory.md` before editing.

Hard boundaries:

```text
AHK owns tray, GUI, hotkeys, startup registration, target-window safety, and final typing.
uvox-capture owns CPAL audio, fast overlay, shell IPC, downloads, and worker supervision.
uvox-infer is the only active process allowed to load parakeet.dll or GGUF models.
```

Do not copy AHK v1 syntax into `ahk/`. Every executable AHK entry point starts with:

```ahk
#Requires AutoHotkey v2.0
#SingleInstance Force
```

Do not reintroduce in-process Parakeet loading under `src/capture/`. Run:

```powershell
.\scripts\test-static.ps1
cargo test --all-targets
```

Use git history as the reference for old monolith behavior; the archived monolith tree is no longer present in the working tree.
