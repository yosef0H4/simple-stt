param(
    [string]$RuntimeUrl = "https://github.com/yosef0H4/parakeet-windows-cuda-build/releases/download/v0.0.1-sm86/parakeet-windows-cuda-sm86.zip",
    [switch]$SkipToolInstall,
    [switch]$SkipRuntime,
    [switch]$SkipBuild,
    [switch]$SkipTests,
    [switch]$FullValidation
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$RuntimeDir = Join-Path $Root "external\parakeet-runtime\parakeet-windows-cuda"
$RuntimeDll = Join-Path $RuntimeDir "bin\parakeet.dll"
$RuntimeModel = Join-Path $RuntimeDir "models\tdt_ctc-110m-f16.gguf"

function Have-Command([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Resolve-AutoHotkey {
    @(
        "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey64.exe",
        "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe",
        "$env:LOCALAPPDATA\Programs\AutoHotkey\v2\AutoHotkey64.exe",
        "$env:LOCALAPPDATA\Programs\AutoHotkey\v2\AutoHotkey.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

function Install-WingetPackage([string]$Id, [string]$Name) {
    if ($SkipToolInstall) {
        throw "$Name is missing. Install it manually or rerun without -SkipToolInstall."
    }
    if (-not (Have-Command winget)) {
        throw "$Name is missing and winget is unavailable. Install $Name manually, then rerun this script."
    }
    Write-Host "Installing $Name with winget..."
    winget install --id $Id --exact --source winget --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) {
        throw "winget failed while installing $Name."
    }
}

function Invoke-ProjectPowerShell([string]$ScriptPath, [string[]]$Arguments = @()) {
    powershell -NoProfile -ExecutionPolicy Bypass -File $ScriptPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$ScriptPath failed with exit code $LASTEXITCODE."
    }
}

function Ensure-Tools {
    if (-not (Have-Command cargo)) {
        Install-WingetPackage "Rustlang.Rustup" "Rust toolchain"
    }
    if (-not (Resolve-AutoHotkey)) {
        Install-WingetPackage "AutoHotkey.AutoHotkey" "AutoHotkey v2"
    }
    if (-not (Have-Command cargo)) {
        throw "cargo is still unavailable. Open a new terminal so PATH changes apply, then rerun this script."
    }
    if (-not (Have-Command python)) {
        Install-WingetPackage "Python.Python.3.12" "Python"
    }
    if (-not (Have-Command python)) {
        throw "python is still unavailable. Open a new terminal so PATH changes apply, then rerun this script."
    }
    if (-not (Resolve-AutoHotkey)) {
        throw "AutoHotkey v2 is still unavailable. Open a new terminal or pass its path to scripts\run-dev.ps1."
    }
}

function Copy-DirectoryContents([string]$Source, [string]$Destination) {
    if (Test-Path -LiteralPath $Destination) {
        Remove-Item -LiteralPath $Destination -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Get-ChildItem -LiteralPath $Source -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $Destination -Recurse -Force
    }
}

function Install-ParakeetRuntime {
    if ((Test-Path -LiteralPath $RuntimeDll) -and (Test-Path -LiteralPath $RuntimeModel)) {
        Write-Host "Parakeet runtime already installed: $RuntimeDir"
        return
    }
    if ($SkipRuntime) {
        throw "Parakeet runtime is missing. Remove -SkipRuntime or place files under $RuntimeDir."
    }

    $TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("simple-stt-bootstrap-" + [guid]::NewGuid().ToString("N"))
    $ZipPath = Join-Path $TempRoot "parakeet-runtime.zip"
    $ExtractDir = Join-Path $TempRoot "extract"
    New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null
    try {
        Write-Host "Downloading Parakeet runtime..."
        Invoke-WebRequest -Uri $RuntimeUrl -OutFile $ZipPath
        Write-Host "Extracting Parakeet runtime..."
        Expand-Archive -LiteralPath $ZipPath -DestinationPath $ExtractDir -Force

        $Dll = Get-ChildItem -LiteralPath $ExtractDir -Recurse -Filter "parakeet.dll" -File |
            Select-Object -First 1
        if (-not $Dll) {
            throw "Downloaded runtime did not contain bin\parakeet.dll."
        }
        $RuntimeRoot = Split-Path -Parent (Split-Path -Parent $Dll.FullName)
        $Model = Join-Path $RuntimeRoot "models\tdt_ctc-110m-f16.gguf"
        if (-not (Test-Path -LiteralPath $Model)) {
            throw "Downloaded runtime did not contain models\tdt_ctc-110m-f16.gguf."
        }

        New-Item -ItemType Directory -Path (Split-Path -Parent $RuntimeDir) -Force | Out-Null
        Copy-DirectoryContents $RuntimeRoot $RuntimeDir
        Write-Host "Installed Parakeet runtime: $RuntimeDir"
    } finally {
        if (Test-Path -LiteralPath $TempRoot) {
            Remove-Item -LiteralPath $TempRoot -Recurse -Force
        }
    }
}

Set-Location $Root
Write-Host "Bootstrapping Simple STT developer environment..."
Ensure-Tools
if (-not $SkipRuntime) {
    Install-ParakeetRuntime
}

Invoke-ProjectPowerShell (Join-Path $PSScriptRoot "check-prereqs.ps1") @("-RequireRuntime")

if (-not $SkipBuild) {
    if ($SkipTests) {
        Invoke-ProjectPowerShell (Join-Path $PSScriptRoot "build-release.ps1") @("-SkipTests")
    } else {
        Invoke-ProjectPowerShell (Join-Path $PSScriptRoot "build-release.ps1")
    }
}

python (Join-Path $PSScriptRoot "verify-static.py")
python (Join-Path $Root "tools\ipc-poc\test_poc.py")

if ($FullValidation) {
    & (Join-Path $PSScriptRoot "test-ahk-full.cmd")
}

Write-Host ""
Write-Host "Bootstrap complete."
Write-Host "Start the development shell with:"
Write-Host "  .\scripts\run-dev.ps1 -SkipBuild"
