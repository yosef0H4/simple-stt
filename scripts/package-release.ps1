param(
    [string]$Ahk2Exe = "$env:ProgramFiles\AutoHotkey\Compiler\Ahk2Exe.exe",
    [string]$OutputDir,
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not $OutputDir) { $OutputDir = Join-Path $Root "artifacts\uvox-package" }
if (-not $SkipBuild) { & (Join-Path $PSScriptRoot "build-release.ps1") }
if (-not (Test-Path -LiteralPath $Ahk2Exe)) { throw "Ahk2Exe not found: $Ahk2Exe" }
if (Test-Path -LiteralPath $OutputDir) { Remove-Item -LiteralPath $OutputDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutputDir | Out-Null
$Shell = Join-Path $OutputDir "uvox-shell.exe"
$CompileArgs = @('/in', (Join-Path $Root 'ahk\uvox.ahk'), '/out', $Shell)
$Icon = Join-Path $Root 'icons\uvox.ico'
if (Test-Path -LiteralPath $Icon) { $CompileArgs += @('/icon', $Icon) }
& $Ahk2Exe @CompileArgs
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Shell)) { throw "Ahk2Exe did not produce $Shell" }
foreach ($Name in @('uvox-capture.exe','uvox-infer.exe','uvoxctl.exe')) {
    Copy-Item -LiteralPath (Join-Path $Root "target\release\$Name") -Destination $OutputDir
}
Copy-Item -LiteralPath (Join-Path $Root 'LICENSE') -Destination $OutputDir
Copy-Item -LiteralPath (Join-Path $Root 'THIRD_PARTY_NOTICES.md') -Destination $OutputDir
New-Item -ItemType Directory -Path (Join-Path $OutputDir 'fixtures') | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'fixtures\parakeet-smoke.wav') -Destination (Join-Path $OutputDir 'fixtures')
@'
Uvox packaged shell

Place or configure the Parakeet Windows runtime and GGUF model before launch.
Default expected adjacent runtime paths:
  external\parakeet-runtime\parakeet-windows-cuda\bin\parakeet.dll
  external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf

Launch uvox-shell.exe. Use its tray icon to open settings.
See docs\packaging.md in the source repository for details.
'@ | Set-Content -LiteralPath (Join-Path $OutputDir 'README.txt') -Encoding UTF8
Write-Host "Packaged: $OutputDir"
