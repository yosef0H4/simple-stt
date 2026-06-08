param(
    [switch]$RequireAhk2Exe,
    [switch]$RequireRuntime
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
function Require-Command([string]$Name) {
    $Command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $Command) { throw "Missing prerequisite command: $Name" }
    Write-Host "${Name}: $($Command.Source)"
}
Require-Command cargo
Require-Command rustc
Require-Command powershell
$AhkCandidates = @(
    "$env:ProgramFiles\AutoHotkey\v2\AutoHotkey.exe",
    "$env:ProgramFiles\AutoHotkey\AutoHotkey.exe"
)
$Ahk = $AhkCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($Ahk) { Write-Host "AutoHotkey v2 candidate: $Ahk" } else { Write-Warning "AutoHotkey v2 executable not found in standard locations; development shell launch will require -AutoHotkey." }
if ($RequireAhk2Exe) {
    $Compiler = "$env:ProgramFiles\AutoHotkey\Compiler\Ahk2Exe.exe"
    if (-not (Test-Path -LiteralPath $Compiler)) { throw "Ahk2Exe not found: $Compiler" }
    Write-Host "Ahk2Exe: $Compiler"
}
$Runtime = Join-Path $Root "external\parakeet-runtime\parakeet-windows-cuda"
$Dll = Join-Path $Runtime "bin\parakeet.dll"
$Model = Join-Path $Runtime "models\tdt_ctc-110m-f16.gguf"
foreach ($Path in @($Dll, $Model)) {
    if (Test-Path -LiteralPath $Path) { Write-Host "Runtime file: $Path" }
    elseif ($RequireRuntime) { throw "Missing runtime file: $Path" }
    else { Write-Warning "Runtime file not present yet: $Path" }
}
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) { Write-Host "nvidia-smi: available" } else { Write-Warning "nvidia-smi not found; VRAM diagnostics will be skipped." }
Write-Host "Prerequisite inspection complete."
