param(
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not $SkipBuild) { & (Join-Path $PSScriptRoot "build-release.ps1") -SkipTests }
Write-Host "Starting isolated process-exit validation with a smoke-test WAV."
& (Join-Path $PSScriptRoot "memory-cleanup-validation.ps1") -IdleSeconds 5
