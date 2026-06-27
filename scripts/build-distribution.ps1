param(
    [switch]$SkipTests,
    [switch]$IncludeModel,
    [string]$AhkBase,
    [string]$Iscc
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Artifacts = Join-Path $Root 'artifacts'
$Portable = Join-Path $Artifacts 'simple-stt-portable'
$Dist = Join-Path $Artifacts 'dist'
$Runtime = Join-Path $Portable 'runtime'
$ParakeetSource = Join-Path $Root 'external\parakeet-runtime\parakeet-windows-cuda'
$ParakeetDest = Join-Path $Runtime 'external\parakeet-runtime\parakeet-windows-cuda'

function Require-File([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Required file is missing: $Path" }
}
function Get-Sha256([string]$Path) {
    $Stream = [System.IO.File]::OpenRead($Path)
    try {
        $Sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            (($Sha.ComputeHash($Stream) | ForEach-Object { $_.ToString('x2') }) -join '').ToUpperInvariant()
        } finally {
            $Sha.Dispose()
        }
    } finally {
        $Stream.Dispose()
    }
}
function Copy-DirectoryExcludingModels([string]$Source, [string]$Destination) {
    if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Get-ChildItem -LiteralPath $Source -Force | Where-Object { $_.Name -ne 'models' } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $Destination -Recurse -Force
    }
    New-Item -ItemType Directory -Path (Join-Path $Destination 'models') -Force | Out-Null
}
function Resolve-Tool([string]$Provided, [string[]]$Candidates, [string]$Label) {
    if ($Provided) { Require-File $Provided; return $Provided }
    foreach ($Candidate in $Candidates) { if (Test-Path -LiteralPath $Candidate -PathType Leaf) { return $Candidate } }
    throw "$Label was not found. Pass its path explicitly."
}
$AhkBase = Resolve-Tool $AhkBase @(
    "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey64.exe",
    "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe",
    "$env:LOCALAPPDATA\Programs\AutoHotkey\v2\AutoHotkey64.exe",
    "$env:LOCALAPPDATA\Programs\AutoHotkey\v2\AutoHotkey.exe"
) 'AutoHotkey v2 base runtime'
$Iscc = Resolve-Tool $Iscc @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) 'Inno Setup compiler (ISCC.exe)'
Require-File (Join-Path $Root 'ahk\simple-stt.ahk')
Require-File (Join-Path $Root 'fixtures\parakeet-smoke.wav')
Require-File (Join-Path $ParakeetSource 'bin\parakeet.dll')
if ($IncludeModel) {
    Require-File (Join-Path $ParakeetSource 'models\tdt_ctc-110m-f16.gguf')
}

$BuildRelease = Join-Path $PSScriptRoot 'build-release.ps1'
if ($SkipTests) {
    & $BuildRelease -SkipTests
} else {
    & $BuildRelease
}

if (Test-Path -LiteralPath $Portable) { Remove-Item -LiteralPath $Portable -Recurse -Force }
New-Item -ItemType Directory -Path $Runtime -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'ahk\simple-stt.ahk') -Destination $Runtime -Force
Copy-Item -LiteralPath (Join-Path $Root 'ahk\lib') -Destination (Join-Path $Runtime 'lib') -Recurse -Force
Copy-Item -LiteralPath $AhkBase -Destination (Join-Path $Runtime 'AutoHotkey64.exe') -Force

foreach ($Name in @('simple-stt-capture.exe','simple-stt-infer.exe','simple-stt-ctl.exe')) {
    Copy-Item -LiteralPath (Join-Path $Root "target\release\$Name") -Destination $Runtime -Force
}
Copy-Item -LiteralPath (Join-Path $Root 'LICENSE') -Destination $Portable -Force
Copy-Item -LiteralPath (Join-Path $Root 'THIRD_PARTY_NOTICES.md') -Destination $Portable -Force
Copy-Item -LiteralPath (Join-Path $Root 'START_HERE.txt') -Destination $Portable -Force
Set-Content -LiteralPath (Join-Path $Portable 'simple-stt.cmd') -Encoding ASCII -Value '@echo off','start "" "%~dp0runtime\AutoHotkey64.exe" "%~dp0runtime\simple-stt.ahk"'
New-Item -ItemType Directory -Path (Join-Path $Runtime 'fixtures') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'fixtures\parakeet-smoke.wav') -Destination (Join-Path $Runtime 'fixtures') -Force
New-Item -ItemType Directory -Path (Split-Path -Parent $ParakeetDest) -Force | Out-Null
if ($IncludeModel) {
    Copy-Item -LiteralPath $ParakeetSource -Destination $ParakeetDest -Recurse -Force
} else {
    Copy-DirectoryExcludingModels $ParakeetSource $ParakeetDest
}
$Required = @(
    'runtime\simple-stt.ahk',
    'runtime\AutoHotkey64.exe',
    'runtime\simple-stt-capture.exe',
    'runtime\simple-stt-infer.exe',
    'runtime\simple-stt-ctl.exe',
    'runtime\fixtures\parakeet-smoke.wav',
    'runtime\external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll'
)
if ($IncludeModel) {
    $Required += 'runtime\external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf'
}
foreach ($RelativePath in $Required) { Require-File (Join-Path $Portable $RelativePath) }

New-Item -ItemType Directory -Path $Dist -Force | Out-Null
$Setup = Join-Path $Dist 'simple-stt-setup.exe'
$Zip = Join-Path $Dist 'simple-stt-portable.zip'
$InnoScript = Join-Path $Artifacts 'simple-stt.iss'
Copy-Item -LiteralPath (Join-Path $Root 'resources\simple-stt.iss') -Destination $InnoScript -Force
foreach ($Output in @($Setup, $Zip)) {
    if (Test-Path -LiteralPath $Output) { Remove-Item -LiteralPath $Output -Force }
}
& $Iscc $InnoScript
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
Require-File $Setup
Get-ChildItem -LiteralPath $Portable | Compress-Archive -DestinationPath $Zip -CompressionLevel Optimal
Require-File $Zip
Write-Host ''
Write-Host 'Distribution build complete:'
foreach ($Output in @($Setup, $Zip)) {
    Write-Host ('  ' + $Output)
    Write-Host ('  SHA256 ' + (Get-Sha256 $Output))
}
