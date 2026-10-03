#!/usr/bin/env python3
"""Static checks for the Linux/Wayland overhaul.

These checks intentionally avoid Rust toolchain requirements so they can run in
minimal CI containers. They do not replace `cargo test --all-targets`.
"""
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
errors: list[str] = []

frontend_check = subprocess.run(
    [sys.executable, str(ROOT / "scripts/verify-settings-frontend.py")],
    cwd=ROOT,
    capture_output=True,
    text=True,
)
if frontend_check.returncode:
    print(frontend_check.stdout, end="")
    print(frontend_check.stderr, end="")
    errors.append("Settings frontend build output is stale or exceeds its size budget")


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
need("src/infer/parakeet_native.rs", "select_vulkan_device", "preferred_vulkan_device", "ggml_backend_dev_type", "GPU mode requires a physical Vulkan GPU", 'libparakeet.so', 'parakeet.so', 'parakeet_capi_load')
need("src/capture/inference_supervisor.rs", 'LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH', 'add_native_library_search_env')
need("src/config.rs", 'CONFIG_SCHEMA_VERSION: u32 = 9', 'pub struct GeneralConfig', 'pub struct AudioConfig', 'parakeet-linux-vulkan', 'parakeet_native_library_candidates', 'screen context requires AI cleanup')
need("src/capture/input_language.rs", "org.kde.KeyboardLayouts", "getLayoutsList", "getLayout", "xkb::get_state", "_XKB_RULES_NAMES", "language_from_layout", "live_keyboard_language")
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
need("src/common/clipboard.rs", "output_bounded", "--foreground", "text/plain;charset=utf-8", "owner.verify()", "dictation superseded", "MAX_READ")
need("src/bin/simple_stt_linux.rs", "acquire_clipboard_lock", "file.try_lock()", "invalidate_delivery_session", "clipboard.verify()", "transcript retained in clipboard")
if "fn read_clipboard(" in linux_shell or "from_millis(80)" in linux_shell or 'Ok("pasted")' in linux_shell:
    errors.append("Linux paste must verify publication and must not restore previous text on a timer")
need("scripts/test-linux-paste.py", "-displayfd", "SIMPLE_STT_PASTE_X11_E2E", 'env.pop("WAYLAND_DISPLAY"', "x11_delayed_paste_end_to_end")
need("docs/linux-wayland.md", 'Shared shortcut fields remain in JSON', 'Same Parakeet backend model', 'simple-stt-linux configure-shortcuts')

need("src/common/clipboard.rs", 'publish_private', '--sensitive')
need("src/common/private_clipboard.rs", 'x-kde-passwordManagerHint', 'INCR', 'MAX_PAYLOAD')
need("src/bin/simple_stt_linux.rs", 'RetryDelivery', 'NewShortcut::new("retry"', 'insert_text_without_clipboard', 'memory_delivery_command')
need("resources/linux-fast-paste.c", '--insert-text', 'ATSPI_ROLE_PASSWORD_TEXT', 'g_dbus_connection_call_sync', 'attempt\\n')
need("scripts/test-linux-clean-clipboard.py", 'test_retry', 'application/x-simple-stt-test', 'UTF8_STRING', 'empty retry cancelled work')

if errors:
    for error in errors:
        print(f"FAIL: {error}")
    sys.exit(1)

print("PASS: Linux static overhaul checks")
