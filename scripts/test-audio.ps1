$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not $env:RUST_LOG) {
    $env:RUST_LOG = "uvox=debug"
}
$Audio = Join-Path $Root "tests\fixtures\parakeet-smoke.wav"
cargo run -p uvox -- transcribe-file --audio $Audio
