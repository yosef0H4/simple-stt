# Linux packages

The Linux release builds produce two x86_64 packages:

- Fedora 44: `simple-stt-0.3.1-4.fc44.x86_64.rpm`
- Arch Linux / Omarchy: `simple-stt-0.3.1-4-x86_64.pkg.tar.zst`

These packages include the app, Settings, paste helper, Arabic fonts and the
Parakeet Vulkan runtime. Speech models are downloaded separately in Settings.
No source checkout, Cargo, Node.js or Python is needed to run the installed app.
A browser, a working ALSA/PipeWire audio setup and the desktop's normal Vulkan
GPU driver are still needed. CPU inference is available too.

## Install

From the directory containing your downloaded package:

```sh
# Fedora 44
sudo dnf install ./simple-stt-0.3.1-4.fc44.x86_64.rpm

# Arch / Omarchy (update the system first, as usual for Arch packages)
sudo pacman -Syu
sudo pacman -U ./simple-stt-0.3.1-4-x86_64.pkg.tar.zst
```

Launch **Simple STT** from the application menu, or run:

```sh
systemctl --user daemon-reload
simple-stt launch
```

On Wayland, the first launch requests desktop shortcut permission. Use
Settings → General → **Set up shortcuts** to reopen the portal configuration.
Previously approved shortcuts may be restored without another permission dialog.

Settings first offers the recommended English and Arabic models, both checked.
Choose Download or Skip; only checked missing models are downloaded, and the
choice is remembered. Once both selected models finish installing, Follow keyboard
assigns English and Arabic to their matching keyboard languages. Selecting one
uses Single model; Skip uses Single model with None. A manual model selection
saved while downloads are pending takes precedence.

Open Settings → Models, download a model, and select it. Then try a short
recording in an ordinary text editor. The package supplies a systemd **user**
service; it does not start a root daemon or enable startup during installation.
To start with your desktop session:

```sh
systemctl --user enable --now simple-stt-linux.service
```

Settings and state use the stable `installed` profile under
`~/.config/simple-stt/instances/installed/` and
`~/.local/share/simple-stt/instances/installed/`. Models default to
`~/.local/share/simple-stt/models/`. XDG directory overrides are respected.
The app binaries and native runtime are read-only package files in
`/usr/lib/simple-stt/`; upgrades preserve user settings and models.

## Switching from a checkout installation

A service created with `install-user-service` overrides the packaged service.
Stop the old service and keep a backup of that override before trying the RPM:

```sh
systemctl --user disable --now simple-stt-linux.service
mv ~/.config/systemd/user/simple-stt-linux.service \
   ~/.config/systemd/user/simple-stt-linux.service.checkout-backup
systemctl --user daemon-reload
systemctl --user start simple-stt-linux.service
```

Run these commands only if that user service file exists. Verify the service now
uses `/usr/bin/simple-stt`, rather than a checkout's `target/release` directory:

```sh
systemctl --user show simple-stt-linux.service -p FragmentPath -p ExecStart
```

The installed profile starts independently; the checkout profile is preserved.
Existing model files can be copied into `~/.local/share/simple-stt/models/`
to avoid downloading them again. Select those models in installed Settings.
Old manually installed application-menu entries may still launch the checkout;
use `simple-stt launch` during testing. Remove or back up the old entries when
you are ready to switch permanently. Rebind any desktop shortcuts that contain
a checkout path to `simple-stt toggle`, `simple-stt cancel`, etc.; the app's
built-in shortcuts can also be configured in Settings.

To go back to the checkout, stop the packaged service, restore the backed-up
user service file, reload systemd and start the service again. Do not delete
user settings or model folders when uninstalling a package.

## Build locally with Podman

Every desktop build must be installed through DNF/Pacman before launching it.
Do not run the app from `target/` or the frontend `dist/` directory. Isolated
automated tests may still use build outputs.

From a checkout with rootless Podman available:

```sh
scripts/package-linux-release.sh fedora
scripts/package-linux-release.sh arch
# or build both
scripts/package-linux-release.sh all
```

The containers install their own build dependencies, build with the locked Cargo
and npm dependencies, compile the full paste helper, and verify the downloaded
Parakeet runtime checksum. The Fedora and Arch binaries are built separately
against each distribution's libraries. Output goes to the ignored
`artifacts/linux-release/` directory; nothing is installed on the host and
nothing is uploaded to GitHub. The matching source archive, build information
and checksums are retained beside the packages.

`python3 scripts/test-linux-package.py` is an installed-package smoke test for
an isolated container. It checks the embedded Settings server, shared-library
closure, Parakeet ABI, paste-helper features and writable user model paths
without accessing a microphone, clipboard or graphical session. Container
smoke tests do not replace a real desktop dictation test on each distribution.

Fedora 44 packages are for Fedora 44; Arch packages are for a current Arch-based
system. They are not universal Linux binaries. Windows packaging remains separate.

## Inspect clipboard buffering

Enable Settings → Advanced → Diagnostics → **Clipboard buffer notices**, then Save. This developer option is off by default. Each current copy shows a buffering notice followed by ready or failed in the existing overlay. New copies replace unfinished buffers. For metadata-only troubleshooting logs, run `journalctl --user -u simple-stt-linux.service --since "5 minutes ago"`; copied content is never included in buffer logs.
