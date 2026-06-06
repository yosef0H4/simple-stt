$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
cargo run -p uvox -- record-test --seconds 5 --output recording-test.wav
