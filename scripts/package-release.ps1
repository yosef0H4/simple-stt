param(
    [switch]$SkipTests,
    [switch]$IncludeModel,
    [string]$AhkBase,
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'build-distribution.ps1') @PSBoundParameters
if (-not $?) { exit 1 }
