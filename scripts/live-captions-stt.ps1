param(
    [ValidateSet("mic", "file", "models")]
    [string]$Mode = "mic",

    [string]$Audio,

    [int]$Seconds = 0,

    [switch]$Json,

    [switch]$FinalOnly,

    [string]$Model,

    [ValidateSet("key", "legal")]
    [string]$LicenseMode,

    [ValidateSet("raw", "masked", "removed")]
    [string]$Profanity = "raw"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Repo = Join-Path $Root "external\windows-live-captions-stt"
$PackageDir = Join-Path $Repo "src\live_captions_stt"

Set-Location $Root

if (-not (Test-Path $Repo)) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw "GitHub CLI is required to clone yosef0H4/windows-live-captions-stt."
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $Root "external") | Out-Null
    gh repo clone yosef0H4/windows-live-captions-stt $Repo
}

New-Item -ItemType Directory -Force -Path $PackageDir | Out-Null
Copy-Item -Force `
    (Join-Path $Repo "__init__.py"), `
    (Join-Path $Repo "cli.py"), `
    (Join-Path $Repo "helper_manager.py"), `
    (Join-Path $Repo "text_normalize.py"), `
    (Join-Path $Repo "windows_live_captions_stt_helper.cs") `
    $PackageDir

$env:PYTHONPATH = "src"
Set-Location $Repo

$argsList = @("-m", "live_captions_stt.cli")
switch ($Mode) {
    "models" {
        $argsList += "direct-models"
    }
    "file" {
        if (-not $Audio) {
            throw "-Audio is required when -Mode file is used."
        }
        $audioPath = Resolve-Path -LiteralPath $Audio
        $argsList += @("direct-recognize", "--audio", $audioPath.Path)
    }
    "mic" {
        $argsList += "direct-mic"
        if ($Seconds -gt 0) {
            $argsList += @("--seconds", $Seconds)
        }
        if ($Json) {
            $argsList += "--json"
        }
        if ($FinalOnly) {
            $argsList += "--final-only"
        }
    }
}

if ($Model) {
    $argsList += @("--model", $Model)
}
if ($LicenseMode) {
    $argsList += @("--license-mode", $LicenseMode)
}
if ($Profanity -and $Mode -ne "models") {
    $argsList += @("--profanity", $Profanity)
}

python @argsList
