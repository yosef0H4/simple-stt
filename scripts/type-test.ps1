$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
cargo run -p uvox -- type-test "Uvox literal Unicode typing test: héllo world."
