param(
    [switch]$SkipBuild,
    [string]$AutoHotkey
)
$ErrorActionPreference = "Stop"
Write-Host "Launching the Uvox shell. Use the tray icon and choose 'Open Settings'."
& (Join-Path $PSScriptRoot "run-dev.ps1") -SkipBuild:$SkipBuild -AutoHotkey $AutoHotkey
