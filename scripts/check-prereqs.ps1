$ErrorActionPreference = "Stop"
$Missing = @()
foreach ($Name in @("git", "uv", "cargo", "nvidia-smi")) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        $Missing += $Name
    } else {
        Write-Host "$Name: OK"
    }
}
if ($Missing.Count -gt 0) {
    throw "Missing prerequisite commands: $($Missing -join ', ')"
}
Write-Host "Prerequisite command check passed. Run .\scripts\first-test.ps1 next."
