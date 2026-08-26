# Simple STT Linux / Wayland overhaul

This branch keeps the original Simple STT memory model but replaces the Windows
AutoHotkey shell with a Rust Linux shell.

## What changed

- **No AutoHotkey on Linux.** The Linux entry point is the Rust binary `simple-stt-linux`.
- **Shared shortcut fields remain in JSON.** Settings displays them read-only on Linux;
  the desktop/compositor owns the actual bindings.
- **Shared Settings UI.** Run `simple-stt-linux settings`; it launches the same
  disposable browser editor used on Windows.
- **Same Parakeet backend model.** `simple-stt-infer` loads the Parakeet C API
  from `parakeet.dll` on Windows or `libparakeet.so` / `parakeet.so` on Linux.
- **Same RAM-saving design.** Only the disposable inference worker loads the
  GGUF model. The capture daemon records audio and supervises the worker. When
  the worker idles out, process exit releases RAM/VRAM.
- **Linux audio via CPAL.** The microphone capture path now builds on Linux too.
- **Persistent microphone preference.** An empty `audio_device_contains` follows
  ALSA's `default` capture PCM. A saved CPAL/ALSA device ID is preferred when
  present and falls back to the system default while absent. Linux polls input
  topology changes, switches back as soon as the preferred device returns, and
  shows the same fallback/restored overlay notices as Windows.
  Linux device labels include the ALSA PCM name so duplicate hardware/plugin
  entries remain distinguishable.
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
  "schema_version": 5,
  "general": { "enabled": true, "recording_mode": "toggle", "record_hotkey": "CapsLock+S" },
  "audio": { "preferred_device_id": "", "gain": 1.0 },
  "speech": {
    "inference_device": "auto",
    "runtime_dir": "external/parakeet-runtime/parakeet-linux",
    "model_dir": "external/parakeet-runtime/models",
    "selected_model_filename": "tdt_ctc-110m-f16.gguf"
  },
  "output": { "delivery_mode": "paste_ctrl_v", "trailing_space": true },
  "diagnostics": { "log_level": "normal" }
}
```

The complete canonical nested schema is documented in `docs/configuration.md`.
Desktop settings still own Linux shortcut assignment.

CPAL uses ALSA as this build's Linux host. On PipeWire or PulseAudio desktops,
ensure the ALSA `default`, `pipewire`, or `pulse` PCM is configured and usable.
Prefer symbolic card PCM IDs (for example `plughw:CARD=Snowball,DEV=0`) over
numeric card-order IDs such as `plughw:CARD=1,DEV=0`, because numeric ALSA card
ordering can change after reconnects or reboots.

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

Ask Simple STT to open the detected desktop shortcut settings:

```bash
target/release/simple-stt-linux configure-shortcuts
```

If no compatible settings application is detected, the command prints the
manual commands below.

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

Settings offers `auto`, `native`, `wtype`, `ydotool`, `xdotool`, and
`clipboard_only`. Automatic mode selects a tool that matches the current
Wayland or X11 session. Type delivery honors the same paced-typing and WPM
settings as Windows. `wl-clipboard` supplies clipboard data but does not inject
keys; `wtype` targets Wayland, `xdotool` targets X11, and `ydotool` supports
both when `ydotoold` is running. The native portal helper is explicit-only
because desktop security correctly requires user consent for input control;
automatic mode does not cause a permission dialog for each transcript.
When Native fast paste is selected, an approved persistent RemoteDesktop
portal restore token is stored in the instance state directory and reused, so
KDE can restore the permission without prompting on every paste.

Delivery has a current mode and a user-selected cycle list. Smart Paste uses
Shift+Insert in regular applications and Ctrl+Shift+V in recognized terminals;
the individual shortcuts remain available under Advanced paste shortcuts.
App overrides can replace delivery for a particular application with Smart Paste,
Type, Clipboard only, or an explicit paste shortcut. Settings can capture the
focused application after a three-second switch window, with manual ID entry as
a fallback for apps that do not expose their identity.
The default cycle contains Automatic + Smart Paste and Automatic + simulated typing. The
searchable picker treats each automation-tool and delivery-mode pair as one
choice. Selecting a row switches temporarily; its separate Cycle checkbox
controls whether the portal shortcut includes it. This allows cycling across
tools as well as paste, typing, terminal paste, and clipboard-only delivery.

The Linux shell copies the transcript to the normal clipboard and primary
selection, sends a paste keystroke, and then restores the old text clipboard.

## System tray

The daemon publishes a freedesktop StatusNotifierItem used natively by KDE.
Left-click opens Settings. The context menu can start or stop recording, open
Settings, unload the speech model, or close Simple STT. Closing shuts down the
capture service cleanly, so systemd's `Restart=on-failure` does not reopen it.

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
