param(
    [switch]$SkipTests,
    [string]$Ahk2Exe,
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
function Resolve-Tool([string]$Provided, [string[]]$Candidates, [string]$Label) {
    if ($Provided) { Require-File $Provided; return $Provided }
    foreach ($Candidate in $Candidates) { if (Test-Path -LiteralPath $Candidate -PathType Leaf) { return $Candidate } }
    throw "$Label was not found. Pass its path explicitly."
}
$Ahk2Exe = Resolve-Tool $Ahk2Exe @(
    "$env:ProgramFiles\AutoHotkey\Compiler\Ahk2Exe.exe",
    "$env:LOCALAPPDATA\Programs\AutoHotkey\Compiler\Ahk2Exe.exe"
) 'Ahk2Exe'
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
Require-File (Join-Path $ParakeetSource 'models\tdt_ctc-110m-f16.gguf')

$BuildRelease = Join-Path $PSScriptRoot 'build-release.ps1'
if ($SkipTests) {
    & $BuildRelease -SkipTests
} else {
    & $BuildRelease
}

if (Test-Path -LiteralPath $Portable) { Remove-Item -LiteralPath $Portable -Recurse -Force }
New-Item -ItemType Directory -Path $Runtime -Force | Out-Null
$Shell = Join-Path $Runtime 'simple-stt.exe'
$SourceScript = Join-Path $Root 'ahk\simple-stt.ahk'
$CompileArgs = @('/in', "`"$SourceScript`"", '/out', "`"$Shell`"", '/base', "`"$AhkBase`"", '/silent', 'verbose')
$Icon = Join-Path $Root 'icons\simple-stt.ico'
if (Test-Path -LiteralPath $Icon) { $CompileArgs += @('/icon', "`"$Icon`"") }
$CompileArgumentLine = $CompileArgs -join ' '
$CompileProcess = Start-Process -FilePath $Ahk2Exe -ArgumentList $CompileArgumentLine -Wait -PassThru
if ($CompileProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $Shell)) { throw "Ahk2Exe failed with exit code $($CompileProcess.ExitCode); expected output: $Shell" }

foreach ($Name in @('simple-stt-capture.exe','simple-stt-infer.exe','simple-stt-ctl.exe')) {
    Copy-Item -LiteralPath (Join-Path $Root "target\release\$Name") -Destination $Runtime -Force
}
Copy-Item -LiteralPath (Join-Path $Root 'LICENSE') -Destination $Portable -Force
Copy-Item -LiteralPath (Join-Path $Root 'THIRD_PARTY_NOTICES.md') -Destination $Portable -Force
Copy-Item -LiteralPath (Join-Path $Root 'START_HERE.txt') -Destination $Portable -Force
Set-Content -LiteralPath (Join-Path $Portable 'simple-stt.cmd') -Encoding ASCII -Value '@echo off','start "" "%~dp0runtime\simple-stt.exe"'
New-Item -ItemType Directory -Path (Join-Path $Runtime 'fixtures') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'fixtures\parakeet-smoke.wav') -Destination (Join-Path $Runtime 'fixtures') -Force
New-Item -ItemType Directory -Path (Split-Path -Parent $ParakeetDest) -Force | Out-Null
Copy-Item -LiteralPath $ParakeetSource -Destination $ParakeetDest -Recurse -Force
$Required = @(
    'runtime\simple-stt.exe',
    'runtime\simple-stt-capture.exe',
    'runtime\simple-stt-infer.exe',
    'runtime\simple-stt-ctl.exe',
    'runtime\fixtures\parakeet-smoke.wav',
    'runtime\external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll',
    'runtime\external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf'
)
foreach ($RelativePath in $Required) { Require-File (Join-Path $Portable $RelativePath) }

New-Item -ItemType Directory -Path $Dist -Force | Out-Null
$Setup = Join-Path $Dist 'simple-stt-setup.exe'
$Zip = Join-Path $Dist 'simple-stt-portable.zip'
foreach ($Output in @($Setup, $Zip)) {
    if (Test-Path -LiteralPath $Output) { Remove-Item -LiteralPath $Output -Force }
}
& $Iscc (Join-Path $Artifacts 'simple-stt.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
Require-File $Setup
Get-ChildItem -LiteralPath $Portable | Compress-Archive -DestinationPath $Zip -CompressionLevel Optimal
Require-File $Zip
Write-Host ''
Write-Host 'Distribution build complete:'
Get-FileHash -Algorithm SHA256 $Setup, $Zip | ForEach-Object {
    Write-Host ('  ' + $_.Path)
    Write-Host ('  SHA256 ' + $_.Hash)
}
