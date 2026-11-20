"""Probe the live Windows tooltip after cargo build --bin overlay-e2e.

Requires Pillow. Pass --arabic to inspect the shared Arabic content.
"""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import subprocess
import sys
import time

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "artifacts/windows-validation"
OUTPUT.mkdir(parents=True, exist_ok=True)
arabic = "--arabic" in sys.argv
reader = "--reader" in sys.argv
suffix = "-ar" if arabic else ""
u = C.WinDLL("user32", use_last_error=True)
u.SetProcessDpiAwarenessContext(C.c_void_p(-4))
u.GetWindowRect.argtypes = [W.HWND, C.POINTER(W.RECT)]
u.IsWindowVisible.argtypes = [W.HWND]
u.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
u.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
callback = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
samples = []
shot = False
shortcut_shot = False
all_shortcuts_shot = False
command = [str(ROOT / "target/debug/overlay-e2e.exe")]
if arabic:
    command.append("--arabic")
if reader:
    command.append("--reader")
with (OUTPUT / ("tooltip-live" + suffix + ".log")).open("w") as log:
    with subprocess.Popen(command, stdout=log, cwd=ROOT) as process:
        started = time.monotonic()
        try:
            while process.poll() is None and time.monotonic() - started < 20:
                found = []

                @callback
                def enum(hwnd, _):
                    pid = W.DWORD()
                    u.GetWindowThreadProcessId(hwnd, C.byref(pid))
                    name = C.create_unicode_buffer(128)
                    u.GetClassNameW(hwnd, name, 128)
                    if pid.value == process.pid and name.value == "tooltips_class32" and u.IsWindowVisible(hwnd):
                        rect = W.RECT()
                        u.GetWindowRect(hwnd, C.byref(rect))
                        found.append((rect.left, rect.top, rect.right, rect.bottom))
                    return True

                u.EnumWindows(enum, 0)
                elapsed = time.monotonic() - started
                samples.append((elapsed, bool(found)))
                if found and 1 < elapsed < 3 and not shot:
                    name = "tooltip-reader" if reader else "tooltip-fixed"
                    ImageGrab.grab(bbox=found[0]).save(OUTPUT / (name + suffix + ".png"))
                    shot = True
                if reader and found and 3.5 < elapsed < 5 and not shortcut_shot:
                    picture = ImageGrab.grab(bbox=found[0])
                    assert picture.width > 3 * picture.height, "shortcut tooltip lost its two-column layout"
                    picture.save(OUTPUT / ("tooltip-shortcuts" + suffix + ".png"))
                    shortcut_shot = True
                if reader and found and 6.5 < elapsed < 8 and not all_shortcuts_shot:
                    ImageGrab.grab(bbox=found[0]).save(OUTPUT / ("tooltip-all-shortcuts" + suffix + ".png"))
                    all_shortcuts_shot = True
                time.sleep(0.03)
            assert process.poll() is not None, "tooltip helper timed out"
            assert process.returncode == 0, "tooltip helper failed"
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
if reader:
    assert shot and shortcut_shot and all_shortcuts_shot, "reader passage or shortcut tooltip was missing"
    print("PASS native reader and shortcut tooltip screenshots")
    sys.exit(0)
active = [visible for elapsed, visible in samples if 1 < elapsed < 4.5]
assert active and all(active), "tooltip disappeared while recording"
assert any(not visible for elapsed, visible in samples if 7.3 < elapsed < 7.8), "tooltip did not hide"
assert any(visible for elapsed, visible in samples if 8.5 < elapsed < 9.5), "tooltip did not reopen"
print(json.dumps({"result": "PASS", "visible_recording_samples": len(active), "unexpected_hidden_samples": sum(not value for value in active), "screenshot": shot}))
