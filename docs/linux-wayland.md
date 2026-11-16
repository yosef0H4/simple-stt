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
- **Portal-only input.** The local reader controller owns a reusable XDG
  RemoteDesktop EIS broker for typing, paste/copy shortcuts and read-along
  keys/clicks. The OpenWhispr-derived helper retains read-only app/selection
  observation and portal shortcuts; it cannot inject through XTest or uinput.

## Installation

For Fedora and Arch/Omarchy, use the installable packages described in
[Linux packages](linux-installation.md). The commands below are for checkout
development, whose settings and runtime paths stay isolated from installed apps.

## Build

For desktop use, build and install the native package:

```bash
scripts/package-linux-release.sh fedora  # or arch
```

Cargo build outputs are for isolated tests only. See the installation guide for
DNF/Pacman installation; do not launch the desktop app from a build directory.

The optional read-only app/selection helper needs X11, GIO and AT-SPI
headers. Clipboard transport uses wl-clipboard on Wayland. Automatic input
requires a RemoteDesktop portal with ConnectToEIS; Unicode typing additionally
requires the compositor's EIS text capability. If unavailable, choose a paste
action or Clipboard rather than an alternative input backend.

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

After installing the package:

```bash
simple-stt daemon
```

The package already supplies the systemd user service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now simple-stt-linux.service
```

## Bind desktop shortcuts

Wayland automatically binds shortcuts through the portal when the app starts.
Use Settings → General → **Set up shortcuts**, or request configuration again:

```bash
simple-stt configure-shortcuts
```

On Wayland this requests the active portal session’s configuration UI. The app
must be running; portal failures appear in Settings instead of opening an
unrelated desktop panel. Previously approved bindings may skip the first-run
dialog. Desktop-managed shortcuts open the detected desktop settings instead.
If no compatible settings application is detected in that mode, the command
prints the manual commands below.

Print the commands:

```bash
simple-stt print-shortcut-commands
```

Bind these in your desktop's shortcut settings:

```text
Toggle dictation: simple-stt toggle
Cancel dictation: simple-stt cancel
Unload model:     simple-stt unload-model
Stop daemon:      simple-stt shutdown
```

Recommended first shortcut: `Super+Alt+Space` for toggle. Caps Lock and true
hold-to-record are intentionally not required for the Linux MVP.

## Paste behavior

Settings offers actions: Type, Smart Paste, Ctrl+V, Ctrl+Shift+V, Shift+Insert
and Clipboard. Legacy backend names in JSON are accepted and normalized to
`auto`; the delivery cycle deduplicates by action. All injected Linux input
uses the local reader controller's XDG RemoteDesktop EIS broker. Hotkey
registration, screenshot capture, clipboard transport and read-only selection
access remain separate operations.

The compositor grants keyboard control once and may restore it using the
private rotating restore token. Pointer control is requested only for a click
action. Revoked grants can require consent again. A denied or unsupported
portal never switches to another injector. Remote HTTP inference endpoints
cannot access the local input broker.

Type sends Unicode through EIS text, honors pacing, stops when canceled and
checks foreground-window identity on KDE. It does not replay uncertain partial
text. The compositor's text-input support determines compatibility with each
application. Page-turn shortcuts use physical key events, with modifiers joined
by `+`, for example `Ctrl+PageDown`. Portal input always acts on the foreground
window; read-along waits for its selected window to return instead of changing
focus. Clicks use compositor logical input regions and fail when coordinates
cannot be safely mapped.

Smart Paste sends Ctrl+V in regular applications and Ctrl+Shift+V in recognized
terminals. The current action and cycle list are independent; application
overrides can choose another action.

With **Keep clipboard clean** enabled, Wayland clipboard transactions save all
formats in memory, publish dictation privately, and send the selected portal
shortcut. They restore the original formats after a completed text transfer and
a quiet period; Shift+Insert also preserves PRIMARY. New copies cancel restore.
Failed backup or unconfirmed transfer keeps the original clipboard and leaves
Retry available. A transfer proves a client read the bytes, not universal editor
insertion. X11 preservation remains unavailable. Clipboard mode explicitly
copies, and preservation-disabled paste retains the dictation for manual paste.

## System tray

The daemon publishes a freedesktop StatusNotifierItem used natively by KDE.
Left-click opens Settings. The context menu can start or stop recording, open
Settings, unload the speech model, or close Simple STT. Closing shuts down the
capture service cleanly, so systemd's `Restart=on-failure` does not reopen it.

With clipboard preservation disabled, publish the text and send the configured
portal shortcut. A failed shortcut leaves the text available for manual paste.

For terminals, use:

```bash
simple-stt toggle --shift-insert
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
