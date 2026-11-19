# Build, development run, and packaging

## One-command Windows distribution build

From the repository root, run:

```cmd
build-distribution.cmd
```

This is the supported release command. It runs Rust tests, builds the release binaries, stages the AutoHotkey shell script with a bundled AutoHotkey v2 runtime, creates the installer, creates the ZIP archive, and prints SHA-256 hashes.

Windows setup installs the app. Download inference runtimes and models explicitly in Settings > Models, as on Linux. Upgrades preserve existing models, runtimes, settings, and startup shortcuts. The installer version comes from Cargo.toml.

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

Windows/Linux installation parity: both leave STT, TTS and OCR model installation
to an explicit Settings action. Windows setup no longer removes an existing
`models` directory or startup shortcuts during an upgrade, and its version is
read from Cargo rather than maintained separately.

Windows and Linux download inference runtimes separately. Windows Vulkan comes
from the upstream Parakeet release. Windows CUDA dictation uses a
[Simple STT build for Blackwell](https://github.com/yosef0H4/simple-stt-runtimes/releases/tag/parakeet-abi6-cuda12.8-sm120-v1)
with CUDA Runtime and cuBLAS fetched directly from NVIDIA. The Windows CUDA
dictation runtime also requires the x64 Microsoft Visual C++ Redistributable.
Settings can download Sherpa CUDA and its pinned dependencies for reading and
OCR on both platforms. [Distribution sources and notices](research/windows-runtime-distribution.md)

Routing, separate provider credentials, language detection and reading use shared
Rust code. Native hotkeys, clipboard access and rendering use each platform's
desktop APIs. Linux manages the API with a user service. Windows can start and
stop an independent background API process from the CLI.

After rebuilding, run `python scripts/test-windows-installer.py --iscc <ISCC.exe>`
to inspect the actual wizard and perform a private installation. The private test
changes only installer identity/output metadata, disables uninstall registration,
leaves the user's installed app alone, and verifies existing model/config files
survive. Its screenshot and logs are saved under
`artifacts/windows-validation/installer-parity`.

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
```

Installer-created Start Menu, Desktop, and Startup shortcuts target `simple-stt.cmd`; the command file launches the bundled AutoHotkey runtime with `runtime\simple-stt.ahk`.

## Prerequisites

The distribution builder expects these tools and files on the Windows developer machine:

```text
Rust stable toolchain with Cargo
AutoHotkey v2 runtime (AutoHotkey64.exe)
Inno Setup 6 compiler (ISCC.exe)
fixtures\parakeet-smoke.wav
```

The source repository intentionally does not track `external\parakeet-runtime\`, `external\parakeet.cpp\`, `external\AutoHotkey\`, or unrelated local reference checkouts. The standard distribution build does not require a local Arabic model; the app downloads the finished GGUF after an explicit Settings action. Users do not need conversion tools.

Publish source-built Parakeet libraries in the companion runtime repository with the pinned build recipe, checksums, and upstream license notices.

The script auto-detects common AutoHotkey v2 and Inno Setup install locations. Override them when needed:

```cmd
build-distribution.cmd -AhkBase "C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe" -Iscc "C:\Users\you\AppData\Local\Programs\Inno Setup 6\ISCC.exe"
```

## Development run

For a fresh Windows clone, bootstrap the developer environment first:

```powershell
.\scripts\bootstrap-dev.ps1
```

The bootstrap script checks or installs Rust, Python, and AutoHotkey v2 when `winget` is available, downloads the official Windows Vulkan library and English Q8 model into `external\parakeet-runtime\`, builds release binaries, and runs source validation. Download the Arabic model in Settings > Models. The Linux Vulkan bootstrap downloads both Q8 models and verifies their checksums; an optional first argument supplies a local Arabic GGUF instead. Use `-SkipToolInstall` to only check tools, `-SkipRuntime` when the runtime is already staged manually, `-SkipTests` for a faster build-only pass, and `-FullValidation` to include the AutoHotkey runtime smoke suite.

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
