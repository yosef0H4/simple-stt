# Build, development run, and packaging

## Development prerequisites

Windows development machine:

```text
Windows 10 or newer
Rust stable toolchain with Cargo
PowerShell
AutoHotkey v2 installed
Ahk2Exe installed only when compiling the shell
Parakeet Windows CUDA runtime
Selected GGUF model
optional: nvidia-smi for VRAM diagnostics
```

Expected runtime layout in a checkout:

```text
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

## Build Rust binaries

```powershell
.\scripts\check-prereqs.ps1
.\scripts\build-release.ps1
```

Expected outputs:

```text
target\release\uvox-capture.exe
target\release\uvox-infer.exe
target\release\uvoxctl.exe
```

## Development run

With AutoHotkey v2 installed:

```powershell
.\scripts\run-dev.ps1
```

The script builds the three Rust binaries unless `-SkipBuild` is supplied, then launches:

```text
ahk\uvox.ahk
```

The shell resolves binaries beside itself first and then under `target\release` / `target\debug`.

## Compile the shell for distribution

Ahk2Exe is the official AutoHotkey script-to-EXE converter. Package the shell with:

```powershell
.\scripts\package-release.ps1 -Ahk2Exe 'C:\Program Files\AutoHotkey\Compiler\Ahk2Exe.exe'
```

Output staging directory:

```text
artifacts\uvox-package\
    uvox-shell.exe
    uvox-capture.exe
    uvox-infer.exe
    uvoxctl.exe
    README.txt
    LICENSE
    THIRD_PARTY_NOTICES.md
    fixtures\
        parakeet-smoke.wav
    icons\                 optional branded icon assets
```

The smoke WAV is copied because model testing depends on it. The Parakeet runtime and model are not copied by default because they are large and may have separate distribution terms. Relative runtime/model paths resolve against the checkout root for `target\debug` or `target\release` development binaries, and against the installed binary directory for staged distributions. The package README documents either an adjacent external runtime layout or an advanced path configured from the shell GUI.

## Start with Windows

The schema-v2 setting writes a per-user startup shortcut:

```text
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Uvox.lnk
```

In development the shortcut launches `uvox.ahk`. In a staged distribution it launches `uvox-shell.exe`. It must never point at the retired monolithic Rust executable.

## Runtime directories

| Content | Location |
| --- | --- |
| Config | `%APPDATA%\uvox\config.json` |
| Shell log | `%LOCALAPPDATA%\uvox\logs\uvox-shell.log` |
| Capture log | `%LOCALAPPDATA%\uvox\logs\uvox-capture.log` |
| Infer log | `%LOCALAPPDATA%\uvox\logs\uvox-infer.log` |
| Capture state file | `%LOCALAPPDATA%\uvox\state\capture-state.json` |
| Models | configured `model_dir` |
| Parakeet DLL | configured runtime dir under `bin\parakeet.dll` |

## Source-only editing environment note

The overhaul was authored in a Linux editing container without Cargo, Rust, PowerShell, AutoHotkey, a microphone, CUDA, or `nvidia-smi`. No Windows binary was built in that environment. Static checks and the dependency-free IPC POC were executed; Windows build and end-to-end validation commands are provided for the target machine.
