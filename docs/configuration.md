# Configuration schema and ownership

`config.json` is Simple STT's canonical, portable configuration. The disposable `simple-stt-settings` browser process is only a visual editor for it. Windows uses `%APPDATA%\simple-stt\instances\<runtime-id>\config.json`; Linux uses the matching XDG config directory. `SIMPLE_STT_CONFIG` overrides the path for development and tests.

The current intentionally breaking schema is version 5:

```json
{
  "schema_version": 5,
  "general": {
    "enabled": true,
    "recording_mode": "hold",
    "record_hotkey": "CapsLock+S",
    "toggle_delivery_hotkey": "CapsLock+D",
    "cancel_hotkey": "CapsLock+A",
    "capslock_behavior": "preserve_tap",
    "start_at_login": false,
    "ui_theme": "auto"
  },
  "audio": { "preferred_device_id": "", "gain": 1.0 },
  "speech": {
    "inference_device": "auto",
    "runtime_dir": "external\\parakeet-runtime\\parakeet-windows-cuda",
    "model_dir": "external\\parakeet-runtime\\parakeet-windows-cuda\\models",
    "selected_model_filename": "tdt_ctc-110m-f16.gguf",
    "idle_worker_timeout_secs": 180,
    "worker_shutdown_grace_ms": 2000
  },
  "output": {
    "delivery_mode": "smart_paste",
    "enabled_delivery_modes": ["smart_paste", "type"],
    "linux_automation_backend": "auto",
    "linux_delivery_cycle": [
      { "backend": "auto", "mode": "smart_paste" },
      { "backend": "auto", "mode": "type" }
    ],
    "paced_typing_enabled": true,
    "typing_speed_wpm": 450,
    "trailing_space": true,
    "remove_punctuation": false,
    "lowercase": false
  },
  "diagnostics": { "log_level": "normal", "diagnostic_overlay": false, "log_transcripts": false }
}
```

There are no version migrations or migration backups. Every parseable file is normalized immediately: exact current fields with valid values are retained, missing or invalid fields receive defaults, and unknown/obsolete fields are dropped. A syntactically malformed file is preserved byte-for-byte and Settings requires an explicit Import or Reset preview followed by Save before replacing it.

Writes use a temporary file, flush it, and atomically replace the destination. Relative runtime/model paths are preserved in JSON and resolved against the runtime root only when used. An unavailable `audio.preferred_device_id` is retained while capture temporarily follows the system default.

The settings server detects external edits with a content hash before Save. A successful Save asks the capture service to reload and emits `configuration_reloaded`; Windows AHK then reapplies its owned hotkeys, startup registration, transforms, and delivery settings.
