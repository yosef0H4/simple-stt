param(
    [switch]$SkipBuild,
    [string]$AutoHotkey
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not $SkipBuild) { & (Join-Path $PSScriptRoot "build-release.ps1") -SkipTests }
if (-not $AutoHotkey) {
    $AutoHotkey = @(
        "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe",
        "$env:ProgramFiles\AutoHotkey\AutoHotkey.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $AutoHotkey -or -not (Test-Path -LiteralPath $AutoHotkey)) {
    throw "AutoHotkey v2 was not found. Pass -AutoHotkey with the v2 executable path."
}
$Script = Join-Path $Root "ahk\simple-stt.ahk"
Write-Host "Launching development shell: $Script"
& $AutoHotkey $Script
