param(
    [switch]$SkipTests,
    [string]$AhkBase,
    [string]$Iscc,
    [string]$CargoTargetDir
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$PackageVersionMatch = [regex]::Match((Get-Content -LiteralPath (Join-Path $Root 'Cargo.toml') -Raw), '(?m)^version\s*=\s*"([^"]+)"')
if (-not $PackageVersionMatch.Success) { throw 'Unable to read package version from Cargo.toml' }
$PackageVersion = $PackageVersionMatch.Groups[1].Value
$Artifacts = Join-Path $Root 'artifacts'
$Portable = Join-Path $Artifacts 'simple-stt-portable'
$Dist = Join-Path $Artifacts 'dist'
$Runtime = Join-Path $Portable 'runtime'
$ResolvedTargetDir = if ($CargoTargetDir) {
    [System.IO.Path]::GetFullPath($CargoTargetDir)
} elseif ($env:CARGO_TARGET_DIR) {
    [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
} else {
    Join-Path $Root 'target'
}

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
Require-File (Join-Path $Root 'ahk\hotkey-recorder.ahk')
Require-File (Join-Path $Root 'ahk\action-replay.ahk')
Require-File (Join-Path $Root 'fixtures\parakeet-smoke.wav')
Require-File (Join-Path $Root 'fixtures\asr\arabic-sa.wav')
$BuildRelease = Join-Path $PSScriptRoot 'build-release.ps1'
if ($SkipTests) {
    & $BuildRelease -SkipTests -CargoTargetDir $ResolvedTargetDir
} else {
    & $BuildRelease -CargoTargetDir $ResolvedTargetDir
}

if (Test-Path -LiteralPath $Portable) { Remove-Item -LiteralPath $Portable -Recurse -Force }
New-Item -ItemType Directory -Path $Runtime -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'ahk\simple-stt.ahk') -Destination $Runtime -Force
Copy-Item -LiteralPath (Join-Path $Root 'ahk\hotkey-recorder.ahk') -Destination $Runtime -Force
Copy-Item -LiteralPath (Join-Path $Root 'ahk\action-replay.ahk') -Destination $Runtime -Force
Copy-Item -LiteralPath (Join-Path $Root 'ahk\lib') -Destination (Join-Path $Runtime 'lib') -Recurse -Force
Copy-Item -LiteralPath (Join-Path $Root 'ahk\icons') -Destination (Join-Path $Runtime 'icons') -Recurse -Force
Copy-Item -LiteralPath $AhkBase -Destination (Join-Path $Runtime 'AutoHotkey64.exe') -Force

foreach ($Name in @('simple-stt-capture.exe','simple-stt-infer.exe','simple-stt-ctl.exe','simple-stt-settings.exe','readerd.exe','readerctl.exe','reader-tts.exe','reader-ocr.exe','reader-hud.exe')) {
    Copy-Item -LiteralPath (Join-Path $ResolvedTargetDir "release\$Name") -Destination $Runtime -Force
}
Copy-Item -LiteralPath (Join-Path $Root 'assets\language\lid.176.ftz') -Destination $Runtime -Force
Copy-Item -LiteralPath (Join-Path $Root 'LICENSE') -Destination $Portable -Force
New-Item -ItemType Directory -Path (Join-Path $Portable 'licenses') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'assets\language\LICENSE-CC-BY-SA-3.0.txt') -Destination (Join-Path $Portable 'licenses') -Force
Copy-Item -LiteralPath (Join-Path $Root 'assets\language\ATTRIBUTION.md') -Destination (Join-Path $Portable 'licenses\fastText-ATTRIBUTION.md') -Force
Copy-Item -LiteralPath (Join-Path $Root 'assets\fonts\LICENSE-NotoSansArabic.txt') -Destination (Join-Path $Portable 'licenses') -Force
Copy-Item -LiteralPath (Join-Path $Root 'assets\fonts\LICENSE-JetBrainsMono.txt') -Destination (Join-Path $Portable 'licenses') -Force
Copy-Item -LiteralPath (Join-Path $Root 'THIRD_PARTY_NOTICES.md') -Destination $Portable -Force
Copy-Item -LiteralPath (Join-Path $Root 'START_HERE.txt') -Destination $Portable -Force
Set-Content -LiteralPath (Join-Path $Portable 'simple-stt.cmd') -Encoding ASCII -Value '@echo off','start "" "%~dp0runtime\AutoHotkey64.exe" "%~dp0runtime\simple-stt.ahk"'
New-Item -ItemType Directory -Path (Join-Path $Runtime 'fixtures') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'fixtures\parakeet-smoke.wav') -Destination (Join-Path $Runtime 'fixtures') -Force
Copy-Item -LiteralPath (Join-Path $Root 'fixtures\asr') -Destination (Join-Path $Runtime 'fixtures\asr') -Recurse -Force
$ModelFiles = @(Get-ChildItem -LiteralPath $Portable -File -Recurse | Where-Object { $_.Extension -in @('.gguf', '.onnx', '.safetensors', '.tflite', '.pt', '.pth') })
if ($ModelFiles.Count) { throw 'Distribution payload unexpectedly contains an inference model' }
$InferenceLibraries = @(Get-ChildItem -LiteralPath $Portable -File -Recurse -Filter '*.dll' | Where-Object { $_.Name -match '^(parakeet|ggml|onnxruntime|sherpa-onnx)' })
if ($InferenceLibraries.Count) { throw 'Distribution payload unexpectedly contains an inference runtime' }
$Required = @(
    'runtime\simple-stt.ahk',
    'runtime\hotkey-recorder.ahk',
    'runtime\action-replay.ahk',
    'runtime\icons\simple-stt.ico',
    'runtime\icons\book.ico',
    'runtime\icons\LICENSE-NOTO-EMOJI.txt',
    'runtime\AutoHotkey64.exe',
    'runtime\simple-stt-capture.exe',
    'runtime\simple-stt-infer.exe',
    'runtime\simple-stt-ctl.exe',
    'runtime\simple-stt-settings.exe',
    'runtime\readerd.exe',
    'runtime\readerctl.exe',
    'runtime\reader-tts.exe',
    'runtime\reader-ocr.exe',
    'runtime\reader-hud.exe',
    'runtime\fixtures\parakeet-smoke.wav'
)
foreach ($RelativePath in $Required) { Require-File (Join-Path $Portable $RelativePath) }

New-Item -ItemType Directory -Path $Dist -Force | Out-Null
$Setup = Join-Path $Dist 'simple-stt-setup.exe'
$Zip = Join-Path $Dist 'simple-stt-portable.zip'
$InnoScript = Join-Path $Artifacts 'simple-stt.iss'
Copy-Item -LiteralPath (Join-Path $Root 'resources\simple-stt.iss') -Destination $InnoScript -Force
foreach ($Output in @($Setup, $Zip)) {
    if (Test-Path -LiteralPath $Output) { Remove-Item -LiteralPath $Output -Force }
}
& $Iscc "/DSimpleSttVersion=$PackageVersion" $InnoScript
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
