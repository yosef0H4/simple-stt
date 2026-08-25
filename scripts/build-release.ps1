param(
    [switch]$SkipTests,
    [string]$CargoTargetDir
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Cargo was not found. Install the stable Rust toolchain first."
}
$ResolvedTargetDir = if ($CargoTargetDir) {
    [System.IO.Path]::GetFullPath($CargoTargetDir)
} elseif ($env:CARGO_TARGET_DIR) {
    [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
} else {
    Join-Path $Root 'target'
}
$env:CARGO_TARGET_DIR = $ResolvedTargetDir
if (-not $SkipTests) {
    cargo test --all-targets
}
cargo build --release --bin simple-stt-capture --bin simple-stt-infer --bin simple-stt-ctl --bin simple-stt-settings
$Expected = @("simple-stt-capture.exe", "simple-stt-infer.exe", "simple-stt-ctl.exe", "simple-stt-settings.exe")
foreach ($Name in $Expected) {
    $Path = Join-Path $ResolvedTargetDir "release\$Name"
    if (-not (Test-Path -LiteralPath $Path)) { throw "Expected binary was not built: $Path" }
    Write-Host "Built: $Path"
}
