# Configuration schema and ownership

`config.json` is Simple STT's canonical, portable configuration. The disposable `simple-stt-settings` browser process is only a visual editor for it. Windows uses `%APPDATA%\simple-stt\instances\<runtime-id>\config.json`; Linux uses the matching XDG config directory. `SIMPLE_STT_CONFIG` overrides the path for development and tests.

The current schema is version 8:

```json
{
  "schema_version": 8,
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
    "runtime_dir": "external\\parakeet-runtime\\parakeet-windows-vulkan",
    "model_dir": "external\\parakeet-runtime\\models",
    "language_mode": "english",
    "english_model_filename": "tdt_ctc-110m-q8_0.gguf",
    "arabic_model_filename": "lemura-arabic-asr-lite-q8_0.gguf",
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
    "app_overrides": [],
    "paced_typing_enabled": true,
    "typing_speed_wpm": 450,
    "trailing_space": true,
    "remove_punctuation": false,
    "lowercase": false
  },
  "diagnostics": { "log_level": "normal", "diagnostic_overlay": false, "log_transcripts": false }
}
```

Every parseable file is normalized immediately: exact current fields with valid values are retained, missing or invalid fields receive defaults, and unknown/obsolete fields are dropped. Schema-v7 `selected_model_filename` migrates into the English slot without replacing a custom choice. The old default Windows CUDA runtime path migrates to Vulkan, and an existing English model is copied into the shared model directory when present. A syntactically malformed file is preserved byte-for-byte and Settings requires an explicit Import or Reset preview followed by Save before replacing it.

`speech.language_mode` is `english`, `arabic`, or `follow_keyboard`. English remains the migration default. Follow-keyboard mode reads the active foreground window's keyboard layout on Windows when recording starts. Linux currently uses the two fixed modes. `speech.inference_device` is `auto`, `gpu`, or `cpu`; old `nvidia_gpu` values are accepted as `gpu`.

Writes use a temporary file, flush it, and atomically replace the destination. Relative runtime/model paths are preserved in JSON and resolved against the runtime root only when used. An unavailable `audio.preferred_device_id` is retained while capture temporarily follows the system default.

`output.delivery_mode` is the method used immediately. On Windows,
`output.enabled_delivery_modes` is the ordered list advanced by the delivery-cycle
hotkey. On Linux, `output.linux_delivery_cycle` is the ordered list because each
choice includes both an automation backend and a delivery method. The active
choice does not have to be part of either cycle. Smart Paste, typing, clipboard
only, and every advanced paste shortcut can be included; both cycle lists must
contain at least one choice.

The settings server detects external edits with a content hash before Save. A successful Save asks the capture service to reload and emits `configuration_reloaded`; Windows AHK then reapplies its owned hotkeys, startup registration, transforms, and delivery settings.
