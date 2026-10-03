#!/usr/bin/env python3
"""Exercise real paste delivery on an isolated X server; never the user desktop."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    for command in ("Xvfb", "xclip", "xdotool", "python3"):
        if not shutil.which(command):
            raise SystemExit(f"Missing paste regression dependency: {command}")
    with tempfile.TemporaryDirectory(prefix="simple-stt-paste-x11-") as folder:
        display_file = Path(folder) / "display"
        with display_file.open("w+") as display:
            server = subprocess.Popen(
                ["Xvfb", "-displayfd", str(display.fileno()), "-screen", "0", "1024x768x24", "-nolisten", "tcp"],
                pass_fds=(display.fileno(),), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            try:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    number = display_file.read_text().strip()
                    if number:
                        break
                    if server.poll() is not None:
                        raise RuntimeError("Isolated X server exited")
                    time.sleep(0.02)
                else:
                    raise RuntimeError("Isolated X server did not become ready")
                env = os.environ.copy()
                env.pop("WAYLAND_DISPLAY", None)
                env.update(DISPLAY=f":{number}", XDG_SESSION_TYPE="x11", SIMPLE_STT_PASTE_X11_E2E="1")
                subprocess.run(
                    ["cargo", "test", "--bin", "simple-stt-linux", "x11_delayed_paste_end_to_end", "--", "--nocapture"],
                    cwd=ROOT, env=env, check=True, timeout=120,
                )
                subprocess.run(
                    ["dbus-run-session", "--", "/usr/bin/python3", "scripts/test-linux-clean-clipboard.py"],
                    cwd=ROOT, env=env, check=True, timeout=60,
                )
                print("PASS: real isolated X11 paste, Unicode, delayed target reads and single insertion")
            finally:
                server.terminate()
                server.communicate(timeout=5)


if __name__ == "__main__":
    main()
