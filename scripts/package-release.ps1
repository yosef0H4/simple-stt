param(
    [switch]$SkipTests,
    [string]$Ahk2Exe,
    [string]$AhkBase,
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'build-distribution.ps1') @PSBoundParameters
if (-not $?) { exit 1 }
