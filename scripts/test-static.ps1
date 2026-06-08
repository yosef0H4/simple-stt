$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
python scripts\verify-static.py
python tools\ipc-poc\test_poc.py
