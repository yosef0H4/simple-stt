$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

Set-Location (Join-Path $Root "worker")
if (-not (Test-Path ".venv")) {
    & (Join-Path $PSScriptRoot "setup-worker.ps1")
}
uv run --no-sync pytest
uv run --no-sync ruff check src tests

Set-Location $Root
cargo test --workspace
