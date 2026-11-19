param(
    [switch]$SkipTests,
    [string]$AhkBase,
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'build-distribution.ps1') @PSBoundParameters
if (-not $?) { exit 1 }
