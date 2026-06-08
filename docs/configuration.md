# Configuration schema and ownership

## Canonical source

Uvox has one canonical persisted config file: human-readable JSON schema version 2.

Default Windows path:

```text
%APPDATA%\uvox\config.json
```

Override for development and diagnostics:

```powershell
$env:UVOX_CONFIG = 'C:\path\to\config.json'
```

The Rust config module owns JSON serialization, migration, validation, and atomic replacement. The AHK GUI reads and writes through local `uvoxctl config-show` / `config-save` commands so the shell does not need a JSON library.

```json
{
  "schema_version": 2,
  "hotkey_enabled": true,
  "record_hotkey": "CapsLock+S",
  "capslock_behavior": "preserve_tap",
  "audio_device_contains": "",
  "audio_gain": 1.0,
  "typing_chunk_chars": 3,
  "typing_interval_ms": 20,
  "trailing_space": true,
  "idle_worker_timeout_secs": 180,
  "worker_shutdown_grace_ms": 2000,
  "start_with_windows": false,
  "log_level": "normal",
  "diagnostic_overlay": false,
  "log_transcripts": false,
  "parakeet_runtime_dir": "external\\parakeet-runtime\\parakeet-windows-cuda",
  "model_dir": "external\\parakeet-runtime\\parakeet-windows-cuda\\models",
  "selected_model_filename": "tdt_ctc-110m-f16.gguf"
}
```

## Fields, validation, and apply scope

| Field | Validation / behavior | Apply scope |
| --- | --- | --- |
| `schema_version` | Must equal `2`. | Migration-owned. |
| `hotkey_enabled` | Boolean. | Immediate shell rebind. |
| `record_hotkey` | Non-empty chord parsed by AHK v2 hotkey parser. | Immediate shell rebind. |
| `capslock_behavior` | `preserve_tap` or `always_off`. | Immediate shell rebind. |
| `audio_device_contains` | Empty for default device or case-insensitive name substring. | Restart capture service. |
| `audio_gain` | Floating point in `(0, 10]`. | Restart capture service. |
| `typing_chunk_chars` | Integer `[1, 256]`. | Immediate for newly received transcripts. |
| `typing_interval_ms` | Integer `0..1000`. | Immediate for newly received transcripts. |
| `trailing_space` | Boolean. | Immediate for newly received transcripts. |
| `idle_worker_timeout_secs` | Positive integer. | Recycle infer worker, keep capture service. |
| `worker_shutdown_grace_ms` | Integer `[250, 30000]`. | Recycle infer worker, keep capture service. |
| `start_with_windows` | Boolean. | Immediate shell shortcut update. |
| `log_level` | `minimal`, `normal`, `debug`, or `extreme`. | Immediate shell filter update; restart capture service and recycle infer worker. |
| `diagnostic_overlay` | Boolean. | Capture reload. |
| `log_transcripts` | Boolean; default false. | Capture reload / worker diagnostics. Transcript content must stay off by default. |
| `parakeet_runtime_dir` | Non-empty path. | Recycle infer worker. |
| `model_dir` | Non-empty path. | Recycle infer worker. |
| `selected_model_filename` | Plain approved `.gguf` filename; no slash, backslash, or `..`. | Recycle infer worker. |

`ReloadConfig` compares old and new audio fields and the capture logging level. The response tells the shell when an audio-service restart is necessary. Model and worker lifecycle fields are applied by recycling only `uvox-infer.exe`.

## Migration from schema 1

On first load of an old config without `schema_version: 2`, Rust:

1. parses known schema-1 fields;
2. maps `idle_timeout_secs` to `idle_worker_timeout_secs`;
3. splits `parakeet_model_path` into `model_dir` and `selected_model_filename`;
4. maps `capslock_always_off` to `capslock_behavior`;
5. normalizes legacy hotkey spelling such as `capslock+s` to `CapsLock+S`;
6. writes a backup named `config.json.schema1.bak`;
7. atomically writes schema 2.

## Atomic writes

On Windows, schema-v2 saves, helper response files, and completed model downloads write a temporary file, flush it, and call `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`. This avoids the old “delete destination then rename” gap. Every model download receives a unique `.partial.<random>` file and removes it after a failed transfer, so overlapping requests do not corrupt one shared partial file.

## Other tracked state

The only additional runtime state is the ephemeral capture discovery file:

```text
%LOCALAPPDATA%\uvox\state\capture-state.json
```

It contains protocol number, capture PID, loopback address, and startup timestamp. The per-launch random token is passed directly from shell to capture/helper command lines and is not persisted as long-lived configuration.
