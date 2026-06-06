$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Missing = @()
foreach ($Name in @("git", "cargo", "nvidia-smi")) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        $Missing += $Name
    } else {
        Write-Host "${Name}: OK"
    }
}
if ($Missing.Count -gt 0) {
    throw "Missing prerequisite commands: $($Missing -join ', ')"
}
$Runtime = Join-Path $Root "external\parakeet-runtime\parakeet-windows-cuda"
$Dll = Join-Path $Runtime "bin\parakeet.dll"
$Model = Join-Path $Runtime "models\tdt_ctc-110m-f16.gguf"
if (-not (Test-Path -LiteralPath $Dll)) {
    throw "Missing native Parakeet DLL: $Dll"
}
if (-not (Test-Path -LiteralPath $Model)) {
    throw "Missing native Parakeet model: $Model"
}
Write-Host "Parakeet runtime: OK"
Write-Host "Prerequisite check passed. Run .\scripts\test-audio.ps1 next."
