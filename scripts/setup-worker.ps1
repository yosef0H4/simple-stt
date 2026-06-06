$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $Root "worker")

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    throw "uv is required. Install it from the official Astral instructions, then rerun this script."
}

Write-Host "Installing Python 3.11 through uv if needed..."
uv python install 3.11

Write-Host "Creating the CUDA worker environment..."
uv venv --python 3.11

Write-Host "Installing PyTorch from the CUDA wheel index; CPU-only PyTorch is not supported."
uv pip install torch torchaudio --index-url https://download.pytorch.org/whl/cu128

Write-Host "Installing worker dependencies."
uv pip install -e .
uv pip install pytest ruff

Write-Host "Checking CUDA and NeMo imports..."
uv run --no-sync uvox-worker doctor --check-nemo
