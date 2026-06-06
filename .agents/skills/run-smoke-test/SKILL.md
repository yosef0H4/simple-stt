# Skill: run the model smoke tests

Use this when validating the CUDA environment or the Nemotron integration.

## Commands

```powershell
.\scripts\setup-worker.ps1
cd worker
uv run --no-sync uvox-worker doctor --check-nemo
uv run --no-sync uvox-worker fetch-sample
uv run --no-sync uvox-worker smoke-test
uv run --no-sync uvox-worker stream-file-test --lookahead-ms 80
```

## Interpret failures

- `torch.cuda.is_available() returned False`: environment failure; do not add CPU fallback.
- Whole-file test fails before output: CUDA, NeMo install, model download, or model compatibility issue.
- Whole-file test passes but stream-file test fails: inspect `nemotron.py` state initialization and chunk processing.
