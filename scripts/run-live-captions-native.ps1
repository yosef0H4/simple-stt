$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not $env:RUST_LOG) {
    $env:RUST_LOG = "uvox=debug"
}
Write-Host "RUST_LOG=$env:RUST_LOG"
cargo run -p uvox -- run-live-captions-native
