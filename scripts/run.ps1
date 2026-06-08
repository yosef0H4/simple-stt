param(
    [switch]$SkipBuild,
    [string]$AutoHotkey
)
$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "run-dev.ps1") -SkipBuild:$SkipBuild -AutoHotkey $AutoHotkey
