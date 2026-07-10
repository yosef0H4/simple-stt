# Simple STT Linux / Wayland overhaul

This branch keeps the original Simple STT memory model but replaces the Windows
AutoHotkey shell with a Rust Linux shell.

## What changed

- **No AutoHotkey on Linux.** The Linux entry point is the Rust binary `simple-stt-linux`.
- **No hotkeys in Linux JSON.** Bind commands in KDE/GNOME/Hyprland shortcut
  settings instead.
- **Same Parakeet backend model.** `simple-stt-infer` loads the Parakeet C API
  from `parakeet.dll` on Windows or `libparakeet.so` / `parakeet.so` on Linux.
- **Same RAM-saving design.** Only the disposable inference worker loads the
  GGUF model. The capture daemon records audio and supervises the worker. When
  the worker idles out, process exit releases RAM/VRAM.
- **Linux audio via CPAL.** The microphone capture path now builds on Linux too.
- **OpenWhispr-style paste helper.** `resources/linux-fast-paste.c` is adapted
  from OpenWhispr and supports terminal-aware paste, Shift+Insert, XTest,
  optional uinput, and optional RemoteDesktop portal paste.

## Build

```bash
cargo build --release
python3 scripts/build-linux-fast-paste.py
```

The paste helper build is best-effort. If `libxtst-dev` / `libX11` headers are
not installed, Simple STT still works but automatic paste falls back to tools
such as `wtype`, `ydotool`, or `xdotool`.

Useful packages on common distros:

```bash
# Debian/Ubuntu
sudo apt install libasound2-dev libx11-dev libxtst-dev wl-clipboard wtype

# Fedora
sudo dnf install alsa-lib-devel libX11-devel libXtst-devel wl-clipboard wtype

# Arch
sudo pacman -S alsa-lib libx11 libxtst wl-clipboard wtype
```

## Runtime layout

Put the Linux Parakeet runtime somewhere like:

```text
external/parakeet-runtime/parakeet-linux/
  bin/libparakeet.so
  # or lib/libparakeet.so, libparakeet.so, parakeet.so

external/parakeet-runtime/models/
  tdt_ctc-110m-f16.gguf
```

The default Linux config points there. You can also use absolute paths in
`~/.config/simple-stt/config.json`:

```json
{
  "schema_version": 4,
  "audio_device_contains": "",
  "audio_gain": 1.0,
  "trailing_space": true,
  "text_delivery_mode": "paste_ctrl_v",
  "remove_punctuation": false,
  "lowercase_output": false,
  "idle_worker_timeout_secs": 180,
  "worker_shutdown_grace_ms": 2000,
  "log_level": "normal",
  "diagnostic_overlay": false,
  "log_transcripts": false,
  "inference_device": "auto",
  "ui_theme": "auto",
  "parakeet_runtime_dir": "external/parakeet-runtime/parakeet-linux",
  "model_dir": "external/parakeet-runtime/models",
  "selected_model_filename": "tdt_ctc-110m-f16.gguf"
}
```

Notice that there is no Linux hotkey field in the example. Desktop settings own
that.

## Start the daemon

From the checkout after building:

```bash
target/release/simple-stt-linux daemon
```

Or install a systemd user service:

```bash
target/release/simple-stt-linux install-user-service
systemctl --user daemon-reload
systemctl --user enable --now simple-stt-linux.service
```

## Bind desktop shortcuts

Print the commands:

```bash
target/release/simple-stt-linux print-shortcut-commands
```

Bind these in your desktop's shortcut settings:

```text
Toggle dictation: /path/to/simple-stt/target/release/simple-stt-linux toggle
Cancel dictation: /path/to/simple-stt/target/release/simple-stt-linux cancel
Unload model:     /path/to/simple-stt/target/release/simple-stt-linux unload-model
Stop daemon:      /path/to/simple-stt/target/release/simple-stt-linux shutdown
```

Recommended first shortcut: `Super+Alt+Space` for toggle. Caps Lock and true
hold-to-record are intentionally not required for the Linux MVP.

## Paste behavior

The Linux shell copies the transcript to the normal clipboard and primary
selection, sends a paste keystroke, and then restores the old text clipboard.

Paste order:

1. `resources/bin/linux-fast-paste --portal` on Wayland when available.
2. `linux-fast-paste --uinput` fallback when available.
3. `wtype`, `ydotool`, or `xdotool` fallback.
4. If automatic paste fails, the transcript remains in the clipboard for manual
   paste.

For terminals, use:

```bash
target/release/simple-stt-linux toggle --shift-insert
```

or bind the stop/toggle shortcut with `--shift-insert` if your terminal drops
`Ctrl+V`.

## Process model

```text
simple-stt-linux daemon
  └── simple-stt-capture        # always-running, lightweight audio + IPC
        └── simple-stt-infer    # disposable Parakeet worker, model lives here
```

The capture process checks for an idle worker every 250 ms. Once the configured
`idle_worker_timeout_secs` passes, it asks the worker to exit. If the worker is
blocked, the supervisor terminates the exact child PID. This is the memory
cleanup guarantee: the OS reclaims model RAM/VRAM when the worker process exits.
