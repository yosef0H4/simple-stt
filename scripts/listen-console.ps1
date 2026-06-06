param(
    [ValidateSet("nemotron", "echo")]
    [string]$Backend = "nemotron"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

Set-Location $Root
cargo run -p uvox -- listen-console --backend $Backend
