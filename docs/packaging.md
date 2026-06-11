# Build, development run, and packaging

## One-command Windows distribution build

From the repository root, run:

```cmd
build-distribution.cmd
```

This is the supported release command. It runs Rust tests, builds the release binaries, compiles the AutoHotkey shell, stages a self-contained portable layout, creates the installer, creates the ZIP archive, and prints SHA-256 hashes.

The implementation lives in `scripts\build-distribution.ps1`. `build-distribution.cmd` is the convenience wrapper and `scripts\package-release.ps1` is now a compatibility wrapper that forwards to the distribution builder.

For a faster local packaging pass after tests have already passed:

```cmd
build-distribution.cmd -SkipTests
```

The native PowerShell entry point is:

```powershell
.\scripts\build-distribution.ps1
```

The legacy compatibility wrapper remains available:

```powershell
.\scripts\package-release.ps1
```

## Build outputs

The one-command build writes:

```text
artifacts\dist\simple-stt-setup.exe
artifacts\dist\simple-stt-portable.zip
```

The portable archive contains the same runtime payload used by the installer:

```text
simple-stt-portable\
    simple-stt.cmd
    LICENSE
    START_HERE.txt
    THIRD_PARTY_NOTICES.md
    runtime\
        simple-stt.exe                      compiled AutoHotkey shell
        simple-stt-capture.exe
        simple-stt-infer.exe
        simple-stt-ctl.exe
        fixtures\parakeet-smoke.wav
        external\parakeet-runtime\parakeet-windows-cuda\
            bin\parakeet.dll
            models\tdt_ctc-110m-f16.gguf
```

## Prerequisites

The distribution builder expects these tools and files on the Windows developer machine:

```text
Rust stable toolchain with Cargo
AutoHotkey v2 compiler (Ahk2Exe.exe)
Inno Setup 6 compiler (ISCC.exe)
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
fixtures\parakeet-smoke.wav
```

The script auto-detects common Ahk2Exe and Inno Setup install locations. Override them when needed:

```cmd
build-distribution.cmd -Ahk2Exe "C:\Program Files\AutoHotkey\Compiler\Ahk2Exe.exe" -Iscc "C:\Users\you\AppData\Local\Programs\Inno Setup 6\ISCC.exe"
```

## Development run

For an editable source run with AutoHotkey installed:

```powershell
.\scripts\run-dev.ps1
```

Runtime data remains under `%APPDATA%\simple-stt` and `%LOCALAPPDATA%\simple-stt`.

## Settings GUI preview during packaging-adjacent UI work

For `ahk\lib\SettingsGui.ahk` changes, validate separately before a full package build:

```bat
python scripts\run-settings-preview.py
```

This lightweight loop runs `/Validate` first and writes `artifacts\gui-loop\report.txt` plus screenshots, which is much faster than rebuilding the full distribution just to catch a GUI syntax or layout regression.
