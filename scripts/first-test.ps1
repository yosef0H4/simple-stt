$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot "setup-worker.ps1")
Set-Location (Join-Path $Root "worker")

Write-Host "`n[1/3] Downloading or reusing the public sample WAV..."
uv run --no-sync uvox-worker fetch-sample

Write-Host "`n[2/3] Running whole-file CUDA Nemotron STT..."
uv run --no-sync uvox-worker smoke-test

Write-Host "`n[3/3] Running the stateful cache-aware streaming path on the same WAV..."
uv run --no-sync uvox-worker stream-file-test --lookahead-ms 80
