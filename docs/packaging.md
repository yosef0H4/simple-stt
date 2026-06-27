# Build, development run, and packaging

## One-command Windows distribution build

From the repository root, run:

```cmd
build-distribution.cmd
```

This is the supported release command. It runs Rust tests, builds the release binaries, stages the AutoHotkey shell script with a bundled AutoHotkey v2 runtime, creates the installer, creates the ZIP archive, and prints SHA-256 hashes.

The default public installer includes the Parakeet runtime DLLs but does not embed a GGUF speech model. It shows a default-checked task to download the recommended `tdt_ctc-110m-f16.gguf` model from Hugging Face during install. If that download fails because the user is offline or the link is unavailable, installation still succeeds and the user can download an approved model later from Settings.

The implementation lives in `scripts\build-distribution.ps1`. `build-distribution.cmd` is the convenience wrapper and `scripts\package-release.ps1` is now a compatibility wrapper that forwards to the distribution builder.

For a faster local packaging pass after tests have already passed:

```cmd
build-distribution.cmd -SkipTests
```

For an explicit offline/private bundle that includes the default model:

```cmd
build-distribution.cmd -IncludeModel
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
        AutoHotkey64.exe                    bundled AutoHotkey v2 runtime
        simple-stt.ahk                      readable AutoHotkey shell entry point
        lib\                                shell support scripts
        simple-stt-capture.exe
        simple-stt-infer.exe
        simple-stt-ctl.exe
        fixtures\parakeet-smoke.wav
        external\parakeet-runtime\parakeet-windows-cuda\
            bin\parakeet.dll
            models\                    empty in the installer payload; optional install task can download a model
```

## Prerequisites

The distribution builder expects these tools and files on the Windows developer machine:

```text
Rust stable toolchain with Cargo
AutoHotkey v2 runtime (AutoHotkey64.exe)
Inno Setup 6 compiler (ISCC.exe)
external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
fixtures\parakeet-smoke.wav
```

The source repository intentionally does not track `external\parakeet-runtime\`, `external\parakeet.cpp\`, `external\AutoHotkey\`, or unrelated local reference checkouts. Developers who do not want to build the Parakeet runtime can place a compatible prebuilt runtime under `external\parakeet-runtime\parakeet-windows-cuda\` before running the distribution builder.

`-IncludeModel` additionally requires:

```text
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

Before publishing a public installer or ZIP that includes `parakeet.dll` or GGUF model files, review the upstream runtime and model licenses and include the required notices/attribution. For source-only GitHub publishing, keep those runtime/model files out of git.

The script auto-detects common AutoHotkey v2 and Inno Setup install locations. Override them when needed:

```cmd
build-distribution.cmd -AhkBase "C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe" -Iscc "C:\Users\you\AppData\Local\Programs\Inno Setup 6\ISCC.exe"
```

## Development run

For a fresh Windows clone, bootstrap the developer environment first:

```powershell
.\scripts\bootstrap-dev.ps1
```

The bootstrap script checks or installs Rust, Python, and AutoHotkey v2 when `winget` is available, downloads the prebuilt Parakeet Windows CUDA runtime into `external\parakeet-runtime\parakeet-windows-cuda\`, builds release binaries, and runs source validation. Use `-SkipToolInstall` to only check tools, `-SkipRuntime` when the runtime is already staged manually, `-SkipTests` for a faster build-only pass, and `-FullValidation` to include the AutoHotkey runtime smoke suite.

For an editable source run after bootstrap:

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
