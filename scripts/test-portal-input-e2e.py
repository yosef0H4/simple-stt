#!/usr/bin/env python3
"""Unattended portal-only input E2E in a private KWin Wayland session.

The harness creates a private D-Bus session, HOME, XDG runtime, virtual KWin,
portal mock, GTK text target, and readerd data directory. It never connects to
or sends input through the user's desktop session.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "scripts/fixtures/linux-unicode"
PORTAL = FIXTURES / "portal_mock.py"
READERD = Path(os.environ.get("SIMPLE_STT_TEST_READERD", ROOT / "target/debug/readerd")).resolve()
OLD_TEST = ROOT / "scripts/test-linux-unicode-input.py"

_spec = importlib.util.spec_from_file_location("linux_unicode_e2e_helpers", OLD_TEST)
_helpers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_helpers)


def require_commands(names):
    missing = [name for name in names if not shutil.which(name)]
    if missing:
        raise SystemExit("Missing portal input E2E dependencies: " + ", ".join(missing))


def wait_for(predicate, label, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise RuntimeError(f"Timed out waiting for {label}")


def stop_process(proc):
    if proc and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)


def read_exact(stream, size):
    chunks = bytearray()
    while len(chunks) < size:
        part = stream.recv(size - len(chunks))
        if not part:
            raise RuntimeError("local IPC closed before a complete reply")
        chunks.extend(part)
    return bytes(chunks)


def ipc(data_dir, request, timeout=30):
    payload = json.dumps(request, ensure_ascii=False, separators=(",", ":")).encode()
    frame = struct.pack("<I", len(payload) + 1) + b"\0" + payload
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(timeout)
        stream.connect(str(data_dir / "simple-tts.sock"))
        stream.sendall(frame)
        length = struct.unpack("<I", read_exact(stream, 4))[0]
        if not 2 <= length <= 64 * 1024:
            raise RuntimeError(f"invalid local IPC frame size {length}")
        if read_exact(stream, 1) != b"\0":
            raise RuntimeError("unexpected binary local IPC response")
        reply = json.loads(read_exact(stream, length - 1))
    return reply


def send_input(data_dir, action, request_id=None, timeout=30):
    request_id = request_id or uuid.uuid4().hex
    return ipc(data_dir, {"op": "input", "id": request_id, "action": action}, timeout), request_id


def expect_input(data_dir, action, label):
    reply, _ = send_input(data_dir, action)
    if not reply.get("ok"):
        raise RuntimeError(f"{label} failed: {reply.get('message')}")
    return reply


def start_daemon(env, config_path, data_dir, log_path):
    log = log_path.open("ab")
    proc = subprocess.Popen(
        [str(READERD), "--config", str(config_path), "daemon", "--foreground", "--no-desktop"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    wait_for(lambda: (data_dir / "simple-tts.sock").exists() or proc.poll() is not None,
             "readerd local IPC socket")
    if proc.poll() is not None:
        log.close()
        raise RuntimeError(f"readerd exited before creating its local IPC socket; see {log_path}")
    return proc, log


def wait_for_text(path, expected, label, timeout=8):
    def matches():
        try:
            return path.read_text(encoding="utf-8") == expected
        except (OSError, UnicodeDecodeError):
            return False
    wait_for(matches, label, timeout)


def focus_gtk_target(ready):
    focus_request = Path(str(ready) + ".focus")
    focus_request.touch()
    wait_for(lambda: not focus_request.exists(), "GTK target to regain input focus")
    time.sleep(0.1)


def assert_portal_counts(log_path, expected_by_capability):
    lines = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
    selected = [line for line in lines if line.startswith("SELECT ")]
    started = [line for line in lines if line.startswith("START ")]
    connected = [line for line in lines if line.startswith("EIS_CONNECTED ")]
    create_calls = [line for line in lines if " CreateSession " in line]
    if len(create_calls) != len(expected_by_capability):
        raise RuntimeError(f"expected {len(expected_by_capability)} unique portal sessions, got {len(create_calls)}: {create_calls!r}")
    if len(selected) != len(expected_by_capability) or len(started) != len(expected_by_capability) or len(connected) != len(expected_by_capability):
        raise RuntimeError(f"portal session lifecycle did not complete once per session: SELECT={selected!r}, START={started!r}, EIS={connected!r}")
    requested = [int(line.rsplit(" requested ", 1)[1].split()[0]) for line in selected]
    granted = [int(line.rsplit(" granted ", 1)[1]) for line in started]
    caps = [int(line.rsplit(" capabilities ", 1)[1].split()[0]) for line in connected]
    if requested != expected_by_capability or granted != expected_by_capability or caps != expected_by_capability:
        raise RuntimeError(f"portal requested/granted/EIS capability masks differ: requested={requested}, granted={granted}, EIS={caps}, expected={expected_by_capability}")
    session_paths = [line.split()[1] for line in selected]
    if len(set(session_paths)) != len(session_paths):
        raise RuntimeError(f"portal reused a session object path: {session_paths!r}")
    return lines


def start_kitty_fixture(env, root, ready_path, expected):
    kitty = shutil.which("kitty")
    if not kitty:
        return None, None
    output_path = root / "kitty-input.json"
    script = root / "kitty-raw-input.py"
    marker = b"\x1b[6~"
    script.write_text(
        "import json, os, sys, termios, tty\n"
        "from pathlib import Path\n"
        "expected = bytes.fromhex(sys.argv[1])\n"
        "out = Path(sys.argv[2])\n"
        "marker = bytes.fromhex(sys.argv[3])\n"
        "fd = sys.stdin.fileno()\n"
        "old = termios.tcgetattr(fd)\n"
        "tty.setraw(fd)\n"
        "Path(sys.argv[4]).write_text('ready', encoding='ascii')\n"
        "data = bytearray()\n"
        "try:\n"
        "    while len(data) < len(expected) or not data.endswith(marker):\n"
        "        part = os.read(fd, 1)\n"
        "        if not part: break\n"
        "        data.extend(part)\n"
        "    out.write_text(json.dumps({'data_hex': bytes(data).hex()}), encoding='utf-8')\n"
        "finally:\n"
        "    termios.tcsetattr(fd, termios.TCSADRAIN, old)\n",
        encoding="utf-8",
    )
    kitty_env = dict(env)
    kitty_env.pop("KITTY_LISTEN_ON", None)
    kitty_env.pop("KITTY_PID", None)
    proc = subprocess.Popen(
        [kitty, "--config", "NONE", "--title", "Simple STT Portal E2E Kitty",
         sys.executable, str(script), expected.hex(), str(output_path), marker.hex(),
         str(ready_path)],
        env=kitty_env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, start_new_session=True,
    )
    wait_for(lambda: ready_path.exists() or proc.poll() is not None,
             "Kitty raw-mode input fixture", timeout=12)
    if proc.poll() is not None and not ready_path.exists():
        raise RuntimeError("optional Kitty fixture exited before becoming ready")
    return proc, output_path


def private_session(root):
    _helpers.require_commands(("kwin_wayland", "wayland-scanner", "pkg-config", "cc", "wl-paste"))
    if not READERD.is_file() or not os.access(READERD, os.X_OK):
        raise RuntimeError(f"Build the readerd binary first: missing executable {READERD}")
    if subprocess.run(["pkg-config", "--exists", "gtk+-3.0"]).returncode != 0:
        raise RuntimeError("Missing GTK 3 development files (pkg-config gtk+-3.0)")

    build_dir = root / "build"
    build_dir.mkdir(parents=True, exist_ok=True)
    injector = _helpers.compile_injector(build_dir)
    target_binary = _helpers.compile_target(build_dir)
    env = _helpers.isolated_env(root, clear_bus=False)
    env.pop("KITTY_LISTEN_ON", None)
    env.pop("KITTY_PID", None)
    runtime = Path(env["XDG_RUNTIME_DIR"])
    data_dir = root / "data" / "reader"
    data_dir.mkdir(parents=True, exist_ok=True)
    data_dir.chmod(0o700)
    config_path = root / "config" / "reader-config.json"
    env.update({
        "SIMPLE_STT_READER_DATA": str(data_dir),
        "SIMPLE_TTS_CONFIG": str(config_path),
        "SIMPLE_STT_CONFIG": str(root / "config" / "app-config.json"),
        "NO_AT_BRIDGE": "1",
        "KWIN_WAYLAND_NO_PERMISSION_CHECKS": "1",
    })

    logs = root / "logs"
    logs.mkdir(exist_ok=True)
    kwin_log_path = logs / "kwin.log"
    portal_log_path = logs / "portal.log"
    target_log_path = logs / "gtk-target.log"
    kwin_log = kwin_log_path.open("wb")
    portal_log = portal_log_path.open("wb")
    target_log = target_log_path.open("wb")
    kwin = portal = target = daemon = kitty = clipboard_owner = None
    daemon_logs = []
    try:
        kwin_env = env.copy()
        kwin_env.pop("QT_QPA_PLATFORM", None)
        kwin_env.pop("GDK_BACKEND", None)
        kwin = subprocess.Popen(
            ["kwin_wayland", "--virtual", "--socket", env["WAYLAND_DISPLAY"],
             "--width", "900", "--height", "600", "--no-global-shortcuts",
             "--no-kactivities", "--no-lockscreen"],
            env=kwin_env, stdin=subprocess.DEVNULL, stdout=kwin_log,
            stderr=subprocess.STDOUT, start_new_session=True,
        )
        wait_for(lambda: (runtime / env["WAYLAND_DISPLAY"]).exists() or kwin.poll() is not None,
                 "private virtual KWin Wayland socket")
        if kwin.poll() is not None:
            raise RuntimeError(f"Private KWin exited during startup; see {kwin_log_path}")

        portal = subprocess.Popen(
            [sys.executable, str(PORTAL), str(injector)], env=env,
            stdin=subprocess.DEVNULL, stdout=portal_log, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        wait_for(lambda: b"READY" in portal_log_path.read_bytes() or portal.poll() is not None,
                 "private portal mock")
        if portal.poll() is not None:
            raise RuntimeError(f"Private portal exited during startup; see {portal_log_path}")

        target_ready = root / "gtk-target.ready"
        target_text = root / "gtk-target.txt"
        target_env = dict(env, NO_AT_BRIDGE="1")
        target = subprocess.Popen(
            [str(target_binary), str(target_ready), str(target_text)], env=target_env,
            stdin=subprocess.DEVNULL, stdout=target_log, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        wait_for(lambda: target_ready.exists() or target.poll() is not None,
                 "GTK editable target")
        if target.poll() is not None:
            raise RuntimeError(f"GTK target failed to start; see {target_log_path}")

        daemon, daemon_log = start_daemon(env, config_path, data_dir, logs / "readerd-first.log")
        daemon_logs.append(daemon_log)
        focus_gtk_target(target_ready)
        expect_input(data_dir, {"kind": "prepare"}, "prepare keyboard session")
        gtk_text = "hello world مرحبا 🙂"
        expect_input(data_dir, {"kind": "text", "text": gtk_text, "delay_ms": 0}, "GTK Unicode typing")
        wait_for_text(target_text, "before\n" + gtk_text, "GTK text entry")
        expect_input(data_dir, {"kind": "key", "key": "Ctrl+A"}, "GTK Ctrl+A action")
        replacement = "keyboard path works"
        expect_input(data_dir, {"kind": "text", "text": replacement, "delay_ms": 0}, "GTK replacement typing")
        wait_for_text(target_text, replacement, "GTK key event and replacement")
        expect_input(data_dir, {"kind": "key", "key": "PageDown"}, "GTK PageDown action")
        expect_input(data_dir, {"kind": "prepare_pointer"}, "prepare pointer session")
        expect_input(data_dir, {"kind": "click", "x": 450, "y": 300}, "private GTK pointer click")
        first_lines = assert_portal_counts(portal_log_path, [1, 2])
        token_path = data_dir / "remote-desktop-keyboard-token"
        wait_for(token_path.exists, "keyboard portal restore token")
        if token_path.read_text(encoding="utf-8") != "private-e2e-token":
            raise RuntimeError("Keyboard portal restore token was not persisted exactly")
        print("PASS GTK Unicode text, key events, and pointer click over two reused portal EIS sessions")

        focus_gtk_target(target_ready)
        prefix = "cancel-prefix-"
        expect_input(data_dir, {"kind": "key", "key": "Ctrl+A"}, "prepare cancellation target")
        expect_input(data_dir, {"kind": "text", "text": prefix, "delay_ms": 0}, "set cancellation prefix")
        wait_for_text(target_text, prefix, "cancellation fixture prefix")
        cancel_text = "NO_REPLAY-" * 12
        cancel_id = "cancel-e2e-" + uuid.uuid4().hex
        request = {"op": "input", "id": cancel_id,
                   "action": {"kind": "text", "text": cancel_text, "delay_ms": 25}}
        with ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(ipc, data_dir, request, 60)
            wait_for(lambda: len(target_text.read_text(encoding="utf-8")) > len(prefix) + 5,
                     "partial canceled typing", timeout=12)
            cancel_reply = ipc(data_dir, {"op": "cancel_input", "id": cancel_id})
            if not cancel_reply.get("ok"):
                raise RuntimeError(f"cancel request failed: {cancel_reply.get('message')}")
            canceled_reply = future.result(timeout=15)
        if canceled_reply.get("ok"):
            raise RuntimeError("mid-typing cancellation unexpectedly succeeded")
        canceled_text = target_text.read_text(encoding="utf-8")
        partial = canceled_text[len(prefix):]
        if not partial or len(partial) >= len(cancel_text) or not cancel_text.startswith(partial):
            raise RuntimeError(f"cancellation did not leave a strict delivered prefix: {partial!r}")
        time.sleep(0.3)
        if target_text.read_text(encoding="utf-8") != canceled_text:
            raise RuntimeError("canceled text continued or replayed after cancellation returned")
        expect_input(data_dir, {"kind": "text", "text": "AFTER-CANCEL", "delay_ms": 0}, "post-cancel keyboard recovery")
        wait_for_text(target_text, canceled_text + "AFTER-CANCEL", "post-cancel text without replay")
        cancel_lines = assert_portal_counts(portal_log_path, [1, 2, 1])
        if not any(line.startswith("SELECT ") and line.endswith("restore True")
                   for line in cancel_lines):
            raise RuntimeError("cancel recovery did not offer the saved portal restore token")
        print("PASS cancellation stops at a text prefix and the next request sends only new text")

        kitty_ready = root / "kitty-target.ready"
        expected_terminal_text = "kitty text مرحبا 🙂".encode("utf-8")
        kitty, kitty_output = start_kitty_fixture(env, root, kitty_ready, expected_terminal_text)
        if kitty:
            wait_for(lambda: kitty_ready.exists() or kitty.poll() is not None,
                     "Kitty raw-mode input fixture ready", timeout=15)
            if kitty.poll() is not None:
                raise RuntimeError("Kitty exited before input delivery")
            expect_input(data_dir, {"kind": "text", "text": expected_terminal_text.decode(), "delay_ms": 0}, "Kitty Unicode text")
            expect_input(data_dir, {"kind": "key", "key": "PageDown"}, "Kitty PageDown action")
            wait_for(lambda: kitty_output.exists(), "Kitty child output", timeout=8)
            actual = bytes.fromhex(json.loads(kitty_output.read_text(encoding="utf-8"))["data_hex"])
            if actual != expected_terminal_text + b"\x1b[6~":
                raise RuntimeError(f"Kitty raw terminal got unexpected UTF-8 or PageDown bytes: {actual!r}")
            print("PASS Kitty raw-mode Unicode text and PageDown escape sequence")
            stop_process(kitty)
            kitty = None
        else:
            print("SKIP Kitty raw-mode fixture: kitty is not installed")

        # Stop and restart the daemon while preserving its private XDG data.
        shutdown = ipc(data_dir, {"op": "shutdown"})
        if not shutdown.get("ok"):
            raise RuntimeError(f"readerd shutdown failed: {shutdown.get('message')}")
        daemon.wait(timeout=10)
        daemon = None
        daemon_log.close()
        focus_gtk_target(target_ready)
        daemon, daemon_log = start_daemon(env, config_path, data_dir, logs / "readerd-restarted.log")
        daemon_logs.append(daemon_log)
        expect_input(data_dir, {"kind": "prepare"}, "prepare restarted keyboard session")
        restarted_expected = target_text.read_text(encoding="utf-8") + " RESTART"
        expect_input(data_dir, {"kind": "text", "text": " RESTART", "delay_ms": 0}, "restored portal typing")
        wait_for_text(target_text, restarted_expected, "typing after daemon restart")
        lines = assert_portal_counts(portal_log_path, [1, 2, 1, 1])
        if sum(line.startswith("SELECT ") and line.endswith("restore True") for line in lines) < 2:
            raise RuntimeError("portal restore token was not reused after cancellation and daemon restart")
        print("PASS portal restore-token reuse across daemon restart")

        # Exercise the existing authenticated retry-delivery CLI against the
        # same private GTK target and compositor; verify every offered MIME
        # format survives its real clipboard transaction.
        # Keep a real Wayland data source alive across focus changes using the
        # existing clipboard provider fixture. GTK's typing target can continue
        # advertising MIME names after focus changes while returning empty data.
        clipboard_snapshot = _helpers.stress_clipboard_snapshot()
        provider_binary = _helpers.compile_slow_clipboard_provider(build_dir)
        provider_ready = root / "retry-clipboard-provider.ready"
        clipboard_owner = subprocess.Popen(
            [str(provider_binary), str(provider_ready)], env=env,
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        _helpers.publish_provider(clipboard_owner, provider_ready, env, injector)
        initial_text = target_text.read_text(encoding="utf-8")
        before_types = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=5)
        before_clipboard = {mime: _helpers.clipboard_data(env, mime)
                            for mime in clipboard_snapshot}
        if before_clipboard != clipboard_snapshot:
            digest = lambda payload: (len(payload), hashlib.sha256(payload).hexdigest())
            raise RuntimeError(
                "retry fixture clipboard did not match expected baseline: "
                f"types={before_types.decode('utf-8', 'replace').splitlines()!r}; "
                f"actual={{{', '.join(f'{mime!r}: {digest(data)!r}' for mime, data in before_clipboard.items())}}}; "
                f"expected={{{', '.join(f'{mime!r}: {digest(data)!r}' for mime, data in clipboard_snapshot.items())}}}"
            )
        try:
            _helpers.run_retry_cli(env, root, target_text, initial_text, clipboard_snapshot, injector)
        except RuntimeError as exc:
            actual = {}
            for mime in clipboard_snapshot:
                try:
                    actual[mime] = _helpers.clipboard_data(env, mime)
                except Exception as clipboard_error:
                    actual[mime] = repr(clipboard_error)
            after_types = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=5)
            digest = lambda payload: ((len(payload), hashlib.sha256(payload).hexdigest())
                                      if isinstance(payload, bytes) else repr(payload))
            raise RuntimeError(
                f"{exc}; clipboard types before={before_types.decode('utf-8', 'replace').splitlines()!r}, "
                f"after={after_types.decode('utf-8', 'replace').splitlines()!r}; "
                f"before={{{', '.join(f'{mime!r}: {digest(data)!r}' for mime, data in before_clipboard.items())}}}; "
                f"after={{{', '.join(f'{mime!r}: {digest(data)!r}' for mime, data in actual.items())}}}"
            ) from exc
        print("PASS retry-delivery CLI restores every original clipboard MIME payload")
    finally:
        stop_process(kitty)
        stop_process(clipboard_owner)
        if daemon and daemon.poll() is None:
            try:
                ipc(data_dir, {"op": "shutdown"}, timeout=5)
                daemon.wait(timeout=5)
            except Exception:
                stop_process(daemon)
        for stream in daemon_logs:
            if not stream.closed:
                stream.close()
        stop_process(target)
        stop_process(portal)
        stop_process(kwin)
        for stream in (kwin_log, portal_log, target_log):
            stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside-session", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--root", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside_session:
        private_session(Path(args.root))
        return
    require_commands(("dbus-run-session",))
    root = Path(tempfile.mkdtemp(prefix="simple-stt-portal-input-e2e-"))
    env = _helpers.isolated_env(root)
    session_log = root / "session.log"
    with session_log.open("w", encoding="utf-8") as log:
        session = subprocess.run(
            ["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()),
             "--inside-session", "--root", str(root)],
            env=env, timeout=240, stdout=log, stderr=subprocess.STDOUT, text=True,
        )
    output = session_log.read_text(encoding="utf-8", errors="replace")
    if session.returncode != 0:
        if output:
            print(output, file=sys.stderr, end="" if output.endswith("\n") else "\n")
        print(f"Private portal-input evidence retained at {root}", file=sys.stderr)
        raise subprocess.CalledProcessError(session.returncode, session.args, output)
    for line in output.splitlines():
        if line.startswith(("PASS", "SKIP")):
            print(line)
    shutil.rmtree(root)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
