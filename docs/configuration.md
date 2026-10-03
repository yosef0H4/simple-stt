# Configuration schema and ownership

`config.json` is Simple STT's canonical, portable configuration. The disposable `simple-stt-settings` browser process is only a visual editor for it. Windows uses `%APPDATA%\simple-stt\instances\<runtime-id>\config.json`; Linux uses the matching XDG config directory. `SIMPLE_STT_CONFIG` overrides the path for development and tests.

The current schema is version 9:

```json
{
  "schema_version": 9,
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
    "selection_mode": "single_model",
    "single_model_filename": null,
    "language_models": {},
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

Every parseable file is normalized immediately: exact current fields with valid values are retained, missing or invalid fields receive defaults, and unknown/obsolete fields are dropped. Schema-v7/v8 fixed English or Arabic selections migrate into `single_model_filename`; Follow keyboard migrates into `language_models.en` and `.ar`. Missing filenames are retained for recovery. The old default Windows CUDA runtime path migrates to Vulkan, and an existing English model is copied into the shared model directory when present. A syntactically malformed file is preserved byte-for-byte and Settings requires an explicit Import or Reset preview followed by Save before replacing it.

`speech.selection_mode` is `single_model` (Use one model) or `follow_keyboard`.
Fresh installations use one model with `single_model_filename: null` (None).
Follow keyboard uses `language_models`, mapping language IDs such as `en`, `ar`,
and `fr` to installed filenames or `null`. Regional keyboard layouts share one
ISO language ID. Unsupported layout IDs remain stable identifiers for manual
assignment. Saved None and missing files silently skip dictation before capture,
overlay, screenshots, or model warm-up. The chosen model and device are frozen
at recording start; changing language alone does not recycle a worker using the
same model.

Windows discovers loaded layouts and locale language codes. Linux reads KDE
Plasma's configured and active layouts on Wayland, or configured XKB layouts and
the effective group on X11. Installed XKB rules XML supplies language metadata,
including variant overrides. Other Wayland desktops show discovery as unavailable;
use one model there. XWayland never supplies a native Wayland target's language.

Audio & recognition contains searchable installed-model choices and model tests.
Every choice includes None and permits any installed model. Follow keyboard ranks
declared language matches first. Refresh languages edits only the draft: new
languages receive a declared match, ordered by recommended, Q8, size, filename,
or None. Existing assignments, including None and removed languages, are retained.
Save activates choices. Model installer handles downloads and removal independently;
installing a model never replaces an assignment, and saved assignments block removal.

`speech.inference_device` is `auto`, `gpu`, or `cpu`; old `nvidia_gpu` values are
accepted as `gpu`. Vulkan GPU selection prefers a physical discrete GPU.

Writes use a temporary file, flush it, and atomically replace the destination. Relative runtime/model paths are preserved in JSON and resolved against the runtime root only when used. An unavailable `audio.preferred_device_id` is retained while capture temporarily follows the system default.

`output.delivery_mode` is the method used immediately. On Windows,
`output.enabled_delivery_modes` is the ordered list advanced by the delivery-cycle
hotkey. On Linux, `output.linux_delivery_cycle` is the ordered list because each
choice includes both an automation backend and a delivery method. The active
choice does not have to be part of either cycle. Smart Paste, typing, clipboard
only, and every advanced paste shortcut can be included; both cycle lists must
contain at least one choice.

The settings server detects external edits with a content hash before Save. A successful Save asks the capture service to reload and emits `configuration_reloaded`; Windows AHK then reapplies its owned hotkeys, startup registration, transforms, and delivery settings.

On Windows and Linux, GPU mode selects a physical Vulkan GPU, preferring a discrete GPU over integrated graphics. It rejects CPU/software Vulkan devices and errors if no physical GPU is available. Automatic mode uses the same preference with CPU fallback. CPU mode always uses the CPU.

## Interface language

`general.ui_language` accepts `auto` (default), `en`, or `ar` within schema 9. Missing/invalid values become `auto`. This setting is independent of keyboard layout, speech model assignments, and transcript content. Settings previews changes immediately; Save applies them to desktop menus and capture notices. System uses Windows user UI language or Linux message-locale preferences, with English fallback.

Canonical messages live in `web/settings/src/lib/locales/`: `en.json`/`ar.json` for Settings and `desktop.en.json`/`desktop.ar.json` for native UI. Use stable keys and named placeholders. `node web/settings/tools/locales.mjs` validates key/placeholder parity and generates `ahk/lib/Locale.ahk`; `npm run build` runs it automatically. Rust embeds the same desktop catalogs. Do not edit the generated AHK catalog directly.
