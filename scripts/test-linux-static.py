#!/usr/bin/env python3
"""Static checks for the Linux/Wayland overhaul.

These checks intentionally avoid Rust toolchain requirements so they can run in
minimal CI containers. They do not replace `cargo test --all-targets`.
"""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
errors: list[str] = []


def need(path: str, *needles: str) -> str:
    p = ROOT / path
    if not p.exists():
        errors.append(f"missing {path}")
        return ""
    body = p.read_text(encoding="utf-8")
    for needle in needles:
        if needle not in body:
            errors.append(f"{path} missing {needle!r}")
    return body


need("Cargo.toml", 'cpal = "0.17"', 'libloading = "0.8"')
need(
    "src/capture/audio.rs",
    'cfg(any(windows, target_os = "linux"))',
    'cpal::default_host',
    'choose_input_device_id',
    '.filter(|device| device.id().is_ok_and',
    'cfg(target_os = "linux")',
)
need("src/infer/parakeet_native.rs", 'libparakeet.so', 'parakeet.so', 'parakeet_capi_load')
need("src/capture/inference_supervisor.rs", 'LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH', 'add_native_library_search_env')
need("src/config.rs", 'CONFIG_SCHEMA_VERSION: u32 = 7', 'pub struct GeneralConfig', 'pub struct AudioConfig', 'parakeet-linux', 'parakeet_native_library_candidates', 'screen context requires AI cleanup')
need("src/capture/process.rs", 'use anyhow::Context;', 'use anyhow::Result;', 'kill')
need("Cargo.toml", 'name = "simple-stt-linux"', 'path = "src/bin/simple_stt_linux.rs"')
linux_shell = need(
    "src/bin/simple_stt_linux.rs",
    'name = "simple-stt-linux"',
    'Toggle',
    'InstallUserService',
    'ConfigureShortcuts',
    'GlobalShortcuts',
    'portal_shortcuts_loop',
    'simple-stt-settings.desktop',
    'wl-copy',
    'wtype',
    'ydotool',
    'xdotool',
)
need("src/capture/overlay_model.rs", 'ascii_visualizer', 'render_overlay_text', 'OverlayPrimary', 'RecordingIndicators')
need("src/capture/overlay_windows.rs", 'render_overlay_text', 'crate::capture::overlay::overlay_model::ascii_visualizer')
need("resources/linux-fast-paste.c", 'adapted from OpenWhispr', 'PASTE_MODE_SHIFT_INSERT', '--detect-terminal', '--active-app', 'active_app_atspi', 'RemoteDesktop')
need("src/bin/simple_stt_linux.rs", 'native-paste-restore-token', '--restore-token', 'run_native_paste')
need("scripts/build-linux-fast-paste.py", 'libx11/libxtst development packages missing', 'HAVE_GIO', 'HAVE_UINPUT')
need("docs/linux-wayland.md", 'Shared shortcut fields remain in JSON', 'Same Parakeet backend model', 'simple-stt-linux configure-shortcuts')

if errors:
    for error in errors:
        print(f"FAIL: {error}")
    sys.exit(1)

print("PASS: Linux static overhaul checks")
