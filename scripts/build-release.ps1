$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
cargo test --workspace
cargo build --release -p uvox
Write-Host "Built: $Root\target\release\uvox.exe"
