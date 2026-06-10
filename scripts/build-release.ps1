param(
    [switch]$SkipTests
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Cargo was not found. Install the stable Rust toolchain first."
}
if (-not $SkipTests) {
    cargo test --all-targets
}
cargo build --release --bin simple-stt-capture --bin simple-stt-infer --bin simple-stt-ctl
$Expected = @("simple-stt-capture.exe", "simple-stt-infer.exe", "simple-stt-ctl.exe")
foreach ($Name in $Expected) {
    $Path = Join-Path $Root "target\release\$Name"
    if (-not (Test-Path -LiteralPath $Path)) { throw "Expected binary was not built: $Path" }
    Write-Host "Built: $Path"
}
