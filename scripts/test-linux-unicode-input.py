#!/usr/bin/env python3
"""Isolated KWin Wayland E2E for clipboard-transaction paste delivery.

This harness always creates a private D-Bus session, XDG home/runtime, and
virtual KWin compositor. It strips inherited desktop/display variables before
starting any child and never reads or writes the user's desktop clipboard.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "scripts/fixtures/linux-unicode"
HELPER = ROOT / "resources/bin/linux-fast-paste"
SHELL = Path(os.environ.get("SIMPLE_STT_TEST_SHELL", ROOT / "target/debug/simple-stt-linux")).resolve()
OLD_SHELL = os.environ.get("SIMPLE_STT_OLD_SHELL")
IMAGE_FORMAT_COUNT = 37
SLOW_FORMATS = tuple(
    f"application/postscript;profile=eps-alias-{index}" for index in range(3)
)


def require_commands(names):
    missing = [name for name in names if not shutil.which(name)]
    if missing:
        raise SystemExit("Missing Linux Unicode E2E dependencies: " + ", ".join(missing))


def wait_for(predicate, label, timeout=20):
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


def checked(args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def compile_injector(build_dir):
    scanner = shutil.which("wayland-scanner")
    if not scanner:
        raise RuntimeError("Missing wayland-scanner (install wayland-protocols/wayland-devel)")
    xml = FIXTURES / "fake_input.xml"
    header = build_dir / "fake_input-client-protocol.h"
    code = build_dir / "fake_input-protocol.c"
    checked([scanner, "client-header", str(xml), str(header)])
    checked([scanner, "private-code", str(xml), str(code)])
    binary = build_dir / "fake-input-keysym"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True).split()
    checked([shutil.which("cc") or "cc", "-O2", "-Wall", "-Wextra", "-Werror",
             "-I", str(build_dir), str(FIXTURES / "fake_input_keysym.c"), str(code),
             "-o", str(binary), *flags])
    return binary


def compile_target(build_dir):
    if subprocess.run(["pkg-config", "--exists", "gtk+-3.0"]).returncode != 0:
        raise RuntimeError("Missing GTK 3 development files (pkg-config gtk+-3.0)")
    binary = build_dir / "gtk-unicode-target"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gtk+-3.0"], text=True).split()
    checked([shutil.which("cc") or "cc", "-O2", "-Wall", "-Wextra", "-Werror",
             str(FIXTURES / "target.c"), "-o", str(binary), *flags])
    return binary


def compile_slow_clipboard_provider(build_dir):
    if subprocess.run(["pkg-config", "--exists", "gtk+-3.0"]).returncode != 0:
        raise RuntimeError("Missing GTK 3 development files (pkg-config gtk+-3.0)")
    binary = build_dir / "slow-clipboard-provider"
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gtk+-3.0"], text=True).split()
    checked([shutil.which("cc") or "cc", "-O2", "-Wall", "-Wextra", "-Werror",
             str(FIXTURES / "slow_clipboard_provider.c"), "-o", str(binary), *flags])
    return binary


def stress_clipboard_snapshot(seed=0):
    mimes = [f"image/x-simple-stt-e2e-{index:02d}" for index in range(IMAGE_FORMAT_COUNT)]
    mimes.extend(SLOW_FORMATS)
    snapshot = {}
    for info, mime in enumerate(mimes):
        snapshot[mime] = bytes((info * 29 + offset * 17 + 3 + seed) & 0xff for offset in range(96))
    snapshot["application/x-simple-stt-e2e"] = bytes((0, 1, *b"Simple-STT clipboard fixture", 255))
    snapshot["image/png"] = bytes.fromhex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
        "0000000b49444154789c636000020000050001a5f645400000000049454e44ae426082")
    snapshot["text/plain;charset=utf-8"] = "original clipboard Ω".encode("utf-8")
    return snapshot


def publish_provider(proc, ready, env, injector):
    wait_for(lambda: Path(str(ready) + ".window").exists() or proc.poll() is not None,
             "clipboard fixture mapped", timeout=8)
    if proc.poll() is not None:
        raise RuntimeError("Clipboard fixture exited before mapping")
    checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
    wait_for(lambda: ready.exists() or proc.poll() is not None,
             "clipboard fixture published after controlled input", timeout=8)


def clipboard_slow_offer_regression(env, root, output_file, initial_text,
                                    provider_binary, clipboard_snapshot, injector):
    stalled_ready = root / "stalled-clipboard-provider.ready"
    stalled_provider = subprocess.Popen([str(provider_binary), str(stalled_ready), "stall"], env=env,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        start_new_session=True)
    stalled = None
    try:
        publish_provider(stalled_provider, stalled_ready, env, injector)
        format_result = {}
        def offered_types_visible():
            result = subprocess.run(["wl-paste", "--list-types"], env=env,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=3)
            if result.returncode == 0:
                format_result["types"] = result.stdout
                return True
            return False
        wait_for(offered_types_visible, "stalled clipboard MIME offer to become visible", timeout=5)
        formats_before = format_result["types"]
        if b"image/x-simple-stt-e2e-00" not in formats_before:
            raise RuntimeError(f"Stalled clipboard offer omitted the expected first image MIME: {formats_before!r}")
        stalled = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        stalled.stdin.write(json.dumps("stalled offer must not replace selection") + "\n")
        stalled.stdin.flush()
        readable, _, _ = __import__("select").select([stalled.stdout], [], [], 12)
        if readable and stalled.stdout.readline().strip() == "ready":
            raise RuntimeError("Clipboard transaction became ready while an offered MIME read was stalled")
        try:
            stalled.wait(timeout=3)
        except subprocess.TimeoutExpired:
            raise RuntimeError("Clipboard transaction did not fail within the stalled-offer bound")
        error = stalled.stderr.read() if stalled.stderr else ""
        if "clipboard backup" not in error.lower() or "stalled" not in error.lower() or "image/x-simple-stt-e2e" not in error:
            raise RuntimeError(f"Stalled clipboard failure did not identify timeout and pending format: {error!r}")
        formats_after = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=5)
        if formats_after != formats_before:
            raise RuntimeError("Clipboard transaction changed the offered selection before all backup reads completed")
    finally:
        stop_process(stalled)
        stop_process(stalled_provider)

    provider_ready = root / "slow-clipboard-provider.ready"
    provider = subprocess.Popen([str(provider_binary), str(provider_ready)], env=env,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        start_new_session=True)
    try:
        publish_provider(provider, provider_ready, env, injector)
        if provider.poll() is not None:
            detail = provider.stderr.read().decode("utf-8", "replace") if provider.stderr else ""
            raise RuntimeError(f"Stress clipboard owner exited before publishing: {detail}")
        for mime, payload in clipboard_snapshot.items():
            if mime in SLOW_FORMATS:
                continue
            if clipboard_data(env, mime) != payload:
                raise RuntimeError(f"Stress clipboard provider published wrong fixture payload for {mime}")

        if OLD_SHELL:
            old_shell = Path(OLD_SHELL).resolve()
            if not old_shell.is_file():
                raise RuntimeError(f"SIMPLE_STT_OLD_SHELL does not exist: {old_shell}")
            old = subprocess.Popen([str(old_shell), "clipboard-transaction"], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
            old.stdin.write(json.dumps("old binary must time out") + "\n")
            old.stdin.flush()
            try:
                readable, _, _ = __import__("select").select([old.stdout], [], [], 30)
                first_line = old.stdout.readline().strip() if readable else ""
                if first_line == "ready":
                    raise RuntimeError("Old clipboard binary became ready despite three serialized slow formats")
                if first_line:
                    raise RuntimeError(f"Old clipboard binary emitted unexpected pre-ready output: {first_line!r}")
                try:
                    old.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    raise RuntimeError("Old clipboard binary neither became ready nor failed within its 30s bound")
                error = old.stderr.read().strip() if old.stderr else ""
                if "reading complete clipboard backup" not in error.lower() and "clipboard backup timed out" not in error.lower():
                    raise RuntimeError(f"Old clipboard binary did not fail in the complete-backup path: {error!r}")
            finally:
                stop_process(old)

        expected = initial_text + "slow clipboard regression paste Ω"
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        owner = clipboard_transaction_paste(env, "slow clipboard regression paste Ω", output_file,
            expected, clipboard_snapshot)
        try:
            for mime, payload in clipboard_snapshot.items():
                if clipboard_data(env, mime) != payload:
                    raise RuntimeError(f"Slow-offer transaction did not restore exact original bytes for {mime}")
        except Exception:
            stop_process(owner)
            raise

        return owner
    finally:
        stop_process(provider)


def clipboard_cache_regression(env, root, output_file, provider_binary, injector):
    """Exercise the resident current-only buffer entirely on the private display."""
    log = (root / "clipboard-cache.log").open("wb")
    watcher = subprocess.Popen([str(SHELL), "clipboard-cache-daemon"], env=env,
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
        start_new_session=True)
    providers, owners = [], []

    def status():
        try:
            result = subprocess.run([str(SHELL), "clipboard-cache-status"], env=env,
                capture_output=True, text=True, timeout=3)
            return json.loads(result.stdout) if result.returncode == 0 else {}
        except (subprocess.TimeoutExpired, json.JSONDecodeError):
            return {}

    def wait_ready(label, newer_than=None, min_formats=0, timeout=25):
        found = {}
        def ready():
            value = status()
            if (value.get("ready") and value.get("formats", 0) >= min_formats
                    and (newer_than is None or value.get("generation", 0) > newer_than)):
                found.update(value)
                return True
            if watcher.poll() is not None:
                raise RuntimeError(f"Clipboard cache exited; see {root / 'clipboard-cache.log'}")
            return False
        wait_for(ready, label, timeout=timeout)
        return found

    def provider(name, stalled=False, alternate=False):
        ready = root / f"cache-{name}.ready"
        args = [str(provider_binary), str(ready)] + (
            ["stall"] if stalled else ["alternate"] if alternate else [])
        provider_env = dict(env, NO_AT_BRIDGE="1")
        proc = subprocess.Popen(args, env=provider_env, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=log, start_new_session=True)
        providers.append(proc)
        publish_provider(proc, ready, env, injector)
        if proc.poll() is not None:
            raise RuntimeError(f"{name} clipboard provider exited")
        return Path(str(ready) + ".requests")

    try:
        baseline = wait_ready("initial background clipboard buffer")
        requests = provider("slow-image")
        snapshot = stress_clipboard_snapshot()
        warm = wait_ready("complete slow image background backup",
                          newer_than=baseline["generation"], min_formats=len(snapshot))
        phases = [event["phase"] for event in warm.get("events", [])
                  if event["generation"] == warm["generation"]]
        if phases != ["buffering", "ready"]:
            raise RuntimeError(f"Missing ordered clipboard buffer diagnostics: {phases!r}")
        before = requests.read_text().splitlines()
        for mime in SLOW_FORMATS:
            if before.count(mime) != 1:
                raise RuntimeError(f"Expected one prefetch for {mime}; requests={before!r}")
        payload = "warm image buffer paste مرحبا"
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        owners.append(clipboard_transaction_paste(env, payload, output_file,
            output_file.read_text() + payload, snapshot, ready_latency_limit=1.5))
        if requests.read_text().splitlines() != before:
            raise RuntimeError("Warm paste reread the original image source instead of using its buffer")
        wait_ready("original image buffer retained after restoration", min_formats=len(snapshot))
        payload = "second warm buffer paste Ω"
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        owners.append(clipboard_transaction_paste(env, payload, output_file,
            output_file.read_text() + payload, snapshot, ready_latency_limit=1.5))

        # Cancel an actively stalled multi-format read with two newer copies.
        previous = status()["generation"]
        stalled_requests = provider("stalled", stalled=True)
        wait_for(lambda: stalled_requests.exists() and bool(stalled_requests.read_text()),
                 "stalled source read to start", timeout=8)
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        newest = "newest clipboard عربي".encode()
        payload = "newest buffer paste Ω"
        submitted = threading.Event()
        pending, pending_errors = [], []
        def pending_paste():
            try:
                pending.append(clipboard_transaction_paste(env, payload, output_file,
                    output_file.read_text() + payload, {"text/plain": newest},
                    request_sent=submitted))
            except Exception as exc:
                pending_errors.append(exc)
        paste_thread = threading.Thread(target=pending_paste)
        paste_thread.start()
        if not submitted.wait(timeout=5):
            raise RuntimeError("Paste request did not start while the old image was transferring")
        started = time.monotonic()
        for value in [b"superseded text buffer", "newest clipboard عربي".encode()]:
            subprocess.run(["wl-copy", "--type", "text/plain"], input=value,
                           env=env, check=True, timeout=5)
        latest = wait_ready("newest copy replaces stalled backup", newer_than=previous, timeout=4)
        elapsed = time.monotonic() - started
        if elapsed > 3:
            raise RuntimeError(f"Superseded stalled backup delayed the newest copy by {elapsed:.3f}s")
        if latest["formats"] >= len(snapshot):
            raise RuntimeError("Stale image formats survived the newest text copy")
        paste_thread.join(timeout=15)
        if paste_thread.is_alive():
            raise RuntimeError("Paste still waited for the obsolete image after a new copy")
        if pending_errors:
            raise pending_errors[0]
        owners.extend(pending)
        # A new image with different bytes must replace the text buffer too.
        requests = provider("replacement-image", alternate=True)
        replacement = stress_clipboard_snapshot(seed=11)
        wait_for(lambda: requests.exists() and SLOW_FORMATS[0] in requests.read_text().splitlines(),
                 "replacement image transfer already in progress", timeout=8)
        payload = "replacement image paste Ω"
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        owners.append(clipboard_transaction_paste(env, payload, output_file,
            output_file.read_text() + payload, replacement, ready_latency_limit=7.5))
        for mime in SLOW_FORMATS:
            if requests.read_text().splitlines().count(mime) != 1:
                raise RuntimeError("Paste duplicated the image transfer that was already warming")

        # A failed background backup is never a valid empty clipboard backup.
        previous = status()["generation"]
        failed_requests = provider("failed-backup", stalled=True)
        wait_for(lambda: failed_requests.exists(), "failed backup read started", timeout=8)
        def backup_failed():
            value = status()
            return (value.get("generation", 0) > previous and not value.get("ready", True)
                    and not value.get("warming", True))
        wait_for(backup_failed, "stalled background backup rejected", timeout=12)
        before = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=3)
        rejected = subprocess.run([str(SHELL), "clipboard-transaction"], env=env,
            input=json.dumps("failed cache must not replace clipboard") + "\n",
            capture_output=True, text=True, timeout=12)
        if "ready" in rejected.stdout.splitlines():
            raise RuntimeError("An incomplete background backup was used as a complete empty cache")
        after = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=3)
        if after != before:
            raise RuntimeError("Failed cached backup replaced the original image selection")
        subprocess.run(["wl-copy", "--type", "text/plain"], input=b"copy race baseline",
                       env=env, check=True, timeout=5)
        wait_ready("valid buffer after failed backup", timeout=5)
        clipboard_newer_copy_race(env, "cached stale transaction")
        clipboard_after_read_copy_race(env, "cached settling race")
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        clipboard_empty_selection_paste(env, "cached empty clipboard Ω", output_file,
            output_file.read_text() + "cached empty clipboard Ω")

        # Disabling prefetch keeps a currently restored selection alive, then
        # releases the service when the user makes another copy.
        original = b"disable keeps original clipboard"
        subprocess.run(["wl-copy", "--type", "text/plain"], input=original,
                       env=env, check=True, timeout=5)
        wait_ready("shutdown fixture warmed", timeout=5)
        payload = "last warm paste Ω"
        owners.append(clipboard_transaction_paste(env, payload, output_file,
            output_file.read_text() + payload, {"text/plain": original}, ready_latency_limit=1.5))
        cache_socket = Path(env["XDG_RUNTIME_DIR"]) / "simple-stt/clipboard-cache.sock"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as control:
            control.connect(str(cache_socket))
            control.sendall(b"disable\n")
        wait_for(lambda: not status().get("ready", True), "prefetch disabled", timeout=5)
        if clipboard_data(env, "text/plain") != original:
            raise RuntimeError("Disabling the buffer discarded the currently restored clipboard")
        subprocess.run(["wl-copy", "--type", "text/plain"], input=b"copy after disabled buffer",
                       env=env, check=True, timeout=5)
        watcher.wait(timeout=5)
        if cache_socket.exists():
            raise RuntimeError("Disabled clipboard buffer left its service socket behind")
        print("PASS: resident clipboard buffer warmed slow image formats once, reused them before paste, "
              "restored every byte, cancelled superseded stalled reads, kept only newest text, "
              "shared in-progress transfers with pending paste, rejected incomplete backups, "
              "respected newer copies during paste, preserved an empty clipboard, "
              "and disabled prefetch without clearing a restored selection")
    finally:
        for proc in owners + providers:
            stop_process(proc)
        stop_process(watcher)
        log.close()


def isolated_env(root, clear_bus=True):
    env = os.environ.copy()
    removed = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY", "XAUTHORITY",
               "AT_SPI_BUS_ADDRESS", "DBUS_SYSTEM_BUS_ADDRESS", "XDG_RUNTIME_DIR",
               "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME",
               "XDG_STATE_HOME", "XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID", "HOME"]
    if clear_bus:
        removed.append("DBUS_SESSION_BUS_ADDRESS")
    for key in removed:
        env.pop(key, None)
    home = root / "home"
    runtime = root / "runtime"
    for folder in (home, runtime, root / "config", root / "data", root / "cache", root / "state"):
        folder.mkdir(parents=True, exist_ok=True)
    runtime.chmod(0o700)
    env.update({
        "HOME": str(home), "XDG_RUNTIME_DIR": str(runtime),
        "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"), "XDG_STATE_HOME": str(root / "state"),
        "XDG_SESSION_TYPE": "wayland", "WAYLAND_DISPLAY": "stt-unicode-e2e",
        "GDK_BACKEND": "wayland", "QT_QPA_PLATFORM": "wayland",
        "GTK_USE_PORTAL": "0", "NO_AT_BRIDGE": "1",
        "LC_ALL": "C.UTF-8", "XDG_CURRENT_DESKTOP": "KDE",
        "XDG_SESSION_DESKTOP": "KDE", "KDE_SESSION_VERSION": "6",
        "KDE_FULL_SESSION": "true",
        "QT_LOGGING_RULES": "kwin_core.debug=true;kwin_utils.debug=true",
    })
    return env


def start_private_accessibility_bus(env):
    launcher_path = "/usr/libexec/at-spi-bus-launcher"
    registry_path = "/usr/libexec/at-spi2-registryd"
    if not Path(launcher_path).is_file() or not Path(registry_path).is_file():
        raise RuntimeError("Private accessibility bridge requires at-spi-bus-launcher and at-spi2-registryd")
    launcher = subprocess.Popen([launcher_path, "--launch-immediately"], env=env,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    address = None
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        result = subprocess.run(["gdbus", "call", "--session", "--dest", "org.a11y.Bus",
            "--object-path", "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress"],
            env=env, capture_output=True, text=True, timeout=2)
        if result.returncode == 0:
            try:
                address = __import__("ast").literal_eval(result.stdout)[0]
            except (ValueError, SyntaxError, IndexError):
                pass
            if address:
                break
        time.sleep(0.05)
    if not address:
        stop_process(launcher)
        raise RuntimeError("Private accessibility bus did not start")
    env["AT_SPI_BUS_ADDRESS"] = address
    registry_env = env.copy()
    registry_env["DBUS_SESSION_BUS_ADDRESS"] = address
    registry = subprocess.Popen([registry_path], env=registry_env,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    time.sleep(0.2)
    env["NO_AT_BRIDGE"] = "0"
    return launcher, registry


def clipboard_data(env, mime, primary=False):
    command = ["wl-paste", "--no-newline"]
    if primary:
        command.append("--primary")
    command.extend(["--type", mime])
    return subprocess.check_output(command, env=env, timeout=5)


def read_child_line(proc, description, timeout=10):
    ready, _, _ = __import__("select").select([proc.stdout], [], [], timeout)
    if not ready:
        raise RuntimeError(f"Timed out waiting for clipboard transaction {description}")
    line = proc.stdout.readline().strip()
    if not line:
        raise RuntimeError(f"Clipboard transaction ended before {description}: {proc.poll()}")
    return line


def instance_id(runtime_root):
    path = str(runtime_root)
    stem = Path(path).name
    sanitized = "".join(ch if ch.isascii() and ch.isalnum() else "-" for ch in stem).strip("-").lower() or "simple-stt"
    hashed = 0xcbf29ce484222325
    for byte in path.encode():
        hashed = ((hashed ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return f"{sanitized}-{hashed:016x}"


def run_retry_cli(env, root, output_file, initial_text, clipboard_snapshot, injector):
    if not SHELL.is_file():
        raise RuntimeError(f"Build the debug Linux shell before this integration test: missing {SHELL}")
    runtime_root = root / "runtime-root"
    runtime_root.mkdir()
    instance = instance_id(runtime_root)
    local_data = Path(env["XDG_DATA_HOME"]) / "simple-stt" / "instances" / instance
    state = local_data / "state"
    state.mkdir(parents=True)
    state_file = state / "linux-capture-state.json"
    token_file = local_data / "linux-token"
    token_file.write_text("private-retry-token\n", encoding="ascii")
    (state / "native-paste-restore-token").write_text("private-e2e-token\n", encoding="ascii")
    (local_data / "linux-recording-session.json").write_text(
        json.dumps({"recording": False, "session_id": 41, "updated_at": 1}), encoding="utf-8")
    config = root / "retry-config.json"
    config.write_text(json.dumps({
        "schema_version": 9,
        "general": {"enabled": True, "ui_language": "en"},
        "output": {
            "delivery_mode": "paste_ctrl_v",
            "linux_automation_backend": "native",
            "preserve_clipboard": True,
            "lowercase": False,
            "remove_punctuation": False,
            "trailing_space": True,
        },
    }), encoding="utf-8")

    messages = []
    errors = []
    server = socket.socket()
    server.bind(("127.0.0.1", 0))
    server.listen()
    server.settimeout(0.1)
    state_file.write_text(json.dumps({
        "protocol": 3, "pid": os.getpid(),
        "address": f"127.0.0.1:{server.getsockname()[1]}", "started_unix_ms": 0,
    }), encoding="utf-8")
    stop = threading.Event()

    def serve():
        while not stop.is_set():
            try:
                conn, _ = server.accept()
            except socket.timeout:
                continue
            try:
                with conn, conn.makefile("rwb") as stream:
                    hello = json.loads(stream.readline())
                    if hello.get("token") != token_file.read_text(encoding="ascii").strip():
                        raise AssertionError("retry CLI used a non-fixture auth token")
                    stream.write(json.dumps({"type": "hello_ack", "protocol": 3,
                                             "service_pid": os.getpid()}).encode() + b"\n")
                    stream.flush()
                    for line in stream:
                        request = json.loads(line)
                        command = request["command"]
                        name = command["name"]
                        messages.append(name)
                        response = {"ok": True, "message": "private retry fixture",
                                    "values": {}, "events": []}
                        if name == "last_delivery":
                            response["values"]["text"] = "retry\nمرحبا 🙂"
                        elif name not in ("cancel", "show_notice"):
                            raise AssertionError(f"unexpected retry fixture command: {name}")
                        stream.write(json.dumps({"type": "response", "request_id": request["request_id"],
                                                 "response": response}).encode() + b"\n")
                        stream.flush()
            except Exception as exc:
                errors.append(exc)

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    shell_env = env.copy()
    shell_env.update({"SIMPLE_STT_CONFIG": str(config),
                      "SIMPLE_STT_RUNTIME_ROOT": str(runtime_root)})
    try:
        checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
        retry = subprocess.run([str(SHELL), "retry-delivery"], env=shell_env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=40)
        if retry.returncode != 0:
            raise RuntimeError("Rust retry-delivery failed: " + retry.stderr.decode("utf-8", "replace"))
        expected = initial_text + "retry\nمرحبا 🙂"
        try:
            wait_for(lambda: output_file.exists() and output_file.read_text(encoding="utf-8") == expected,
                     "Rust retry-delivery paste into GTK", timeout=10)
        except RuntimeError as exc:
            actual = output_file.read_text(encoding="utf-8") if output_file.exists() else "<missing>"
            raise RuntimeError(f"{exc}; actual={actual!r}; stdout={retry.stdout.decode('utf-8', 'replace')!r}; "
                               f"stderr={retry.stderr.decode('utf-8', 'replace')!r}; commands={messages}")
        if messages.count("last_delivery") != 1 or "cancel" not in messages or "show_notice" not in messages:
            raise RuntimeError(f"Rust retry CLI did not use the expected authenticated fixture commands: {messages}")
        for mime, payload in clipboard_snapshot.items():
            if clipboard_data(env, mime) != payload:
                raise RuntimeError(f"Rust retry-delivery changed clipboard MIME {mime}")
        if errors:
            raise RuntimeError(f"Rust retry shell fixture error: {errors[0]}")
    finally:
        stop.set()
        thread.join(timeout=2)
        server.close()


def clipboard_transaction_paste(env, text, output_file, expected_text, snapshot, terminal=False,
                               dispatch_delay=0.0, ready_latency_limit=None, request_sent=None):
    owner = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    try:
        ready_started = time.monotonic()
        owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
        owner.stdin.flush()
        if request_sent is not None:
            request_sent.set()
        if read_child_line(owner, "ready", timeout=25) != "ready":
            raise RuntimeError("Clipboard transaction did not become ready")
        ready_elapsed = time.monotonic() - ready_started
        if ready_latency_limit is not None and ready_elapsed > ready_latency_limit:
            raise RuntimeError(f"Warm clipboard ready took {ready_elapsed:.3f}s; limit {ready_latency_limit}s")
        owner.stdin.write("arm\n")
        owner.stdin.flush()
        if read_child_line(owner, "armed") != "armed":
            raise RuntimeError("Clipboard transaction did not arm before shortcut dispatch")
        if dispatch_delay:
            time.sleep(dispatch_delay)
        if not terminal:
            focus_request = output_file.parent / "target.ready.focus"
            focus_request.touch()
            wait_for(lambda: not focus_request.exists(), "private editor focus acknowledged", timeout=3)
            time.sleep(0.2)
        command = [str(HELPER), "--portal", "--restore-token", "private-e2e-token"]
        if terminal:
            command.append("--terminal")
        pasted = subprocess.run(command, env=env, capture_output=True, timeout=30)
        if pasted.returncode != 0:
            owner.stdin.write("abort\n")
            owner.stdin.flush()
            status = read_child_line(owner, "abort restoration", timeout=4)
            raise RuntimeError(f"Paste shortcut failed: rc={pasted.returncode}, "
                               f"stdout={pasted.stdout!r}, stderr={pasted.stderr!r}, owner={status}")
        owner.stdin.write("sent\n")
        owner.stdin.flush()
        status = read_child_line(owner, "transfer completion/restoration", timeout=12)
        if status != "restored":
            subprocess.run(["spectacle", "--background", "--nonotify", "--fullscreen", "--output", str(output_file.parent / "failed-paste.png")], env=env, capture_output=True, timeout=10)
            raise RuntimeError(f"Clipboard transaction ended as {status!r}")
        try:
            wait_for(lambda: output_file.exists() and output_file.read_text(encoding="utf-8") == expected_text,
                     "paste target received exact mixed Unicode text", timeout=8)
        except RuntimeError as exc:
            actual = output_file.read_bytes() if output_file.exists() else b"<missing>"
            raise RuntimeError(f"{exc}; captured target bytes={actual!r}")
        for mime, payload in snapshot.items():
            if clipboard_data(env, mime) != payload:
                raise RuntimeError(f"Clipboard transaction did not restore original MIME {mime}")
        # Do not terminate a restored selection owner: it serves original MIME
        # data until a later copy replaces it or the private compositor exits.
        return owner
    except Exception:
        if owner.poll() is None:
            try:
                owner.stdin.write("abort\n")
                owner.stdin.flush()
                read_child_line(owner, "failure restoration", timeout=3)
            except Exception:
                pass
        raise


def clipboard_empty_selection_paste(env, text, output_file, expected_text):
    subprocess.run(["wl-copy", "--clear"], env=env, check=True, timeout=8)
    wait_for(
        lambda: subprocess.run(["wl-paste", "--no-newline"], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=3).returncode != 0,
        "empty private clipboard before backup",
    )
    owner = clipboard_transaction_paste(env, text, output_file, expected_text, {})
    try:
        owner.wait(timeout=3)
        result = subprocess.run(["wl-paste", "--no-newline"], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=3)
        if result.returncode == 0 and result.stdout:
            raise RuntimeError("Empty clipboard was not restored after the paste transaction")
    finally:
        stop_process(owner)


def clipboard_newer_copy_race(env, text):
    owner = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
    owner.stdin.flush()
    if read_child_line(owner, "race-ready") != "ready":
        raise RuntimeError("Clipboard transaction was not ready for replacement race")
    replacement = "newer user copy Ω"
    subprocess.run(["wl-copy", "--type", "text/plain"], input=replacement.encode("utf-8"),
                   env=env, check=True, timeout=8)
    status = read_child_line(owner, "newer-copy detection", timeout=5)
    if status != "replaced":
        raise RuntimeError(f"Clipboard owner did not yield to a newer copy: {status!r}")
    owner.wait(timeout=3)
    actual = clipboard_data(env, "text/plain")
    if actual != replacement.encode("utf-8"):
        raise RuntimeError(f"Clipboard restore overwrote the newer user copy: {actual!r}")


def clipboard_delayed_read(env, text, snapshot):
    owner = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
    owner.stdin.flush()
    if read_child_line(owner, "delayed-read ready") != "ready":
        raise RuntimeError("Clipboard transaction was not ready for delayed read")
    owner.stdin.write("arm\n")
    owner.stdin.flush()
    if read_child_line(owner, "delayed-read armed") != "armed":
        raise RuntimeError("Clipboard transaction failed to arm for delayed read")
    owner.stdin.write("sent\n")
    owner.stdin.flush()
    readable, _, _ = __import__("select").select([owner.stdout], [], [], 1.2)
    if readable:
        raise RuntimeError(f"Transaction reported before clipboard data was requested: {owner.stdout.readline().strip()!r}")
    copied = subprocess.check_output(["wl-paste", "--no-newline", "--type", "text/plain;charset=utf-8"],
                                     env=env, timeout=5)
    if copied != text.encode("utf-8"):
        raise RuntimeError(f"Delayed clipboard request returned wrong text: {copied!r}")
    status = read_child_line(owner, "delayed-read restoration", timeout=5)
    if status != "restored":
        raise RuntimeError(f"Delayed clipboard transfer ended as {status!r}")
    for mime, payload in snapshot.items():
        if clipboard_data(env, mime) != payload:
            raise RuntimeError(f"Delayed read did not restore original {mime}")


def clipboard_after_read_copy_race(env, text):
    owner = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
    owner.stdin.flush()
    if read_child_line(owner, "settling-race ready") != "ready":
        raise RuntimeError("Clipboard transaction was not ready for settling race")
    owner.stdin.write("arm\nsent\n")
    owner.stdin.flush()
    if read_child_line(owner, "settling-race armed") != "armed":
        raise RuntimeError("Clipboard transaction failed to arm for settling race")
    publisher = subprocess.Popen(["wl-copy", "--foreground", "--type", "text/plain"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    copied = subprocess.check_output(["wl-paste", "--no-newline", "--type", "text/plain;charset=utf-8"],
                                     env=env, timeout=5)
    if copied != text.encode("utf-8"):
        raise RuntimeError(f"Settling-race read returned wrong transcript: {copied!r}")
    replacement = "copy during settle Ω".encode("utf-8")
    publisher.stdin.write(replacement)
    publisher.stdin.close()
    wait_for(lambda: clipboard_data(env, "text/plain") == replacement,
             "newer copy during settle window", timeout=3)
    status = read_child_line(owner, "settling-race replacement", timeout=3)
    if status != "replaced":
        raise RuntimeError(f"Restoration overwrote copy made during settle interval: {status!r}")
    owner.wait(timeout=3)
    if clipboard_data(env, "text/plain") != replacement:
        raise RuntimeError("Clipboard owner overwrote newer copy after reporting replacement")


def clipboard_no_read_abort(env, text, snapshot):
    owner = subprocess.Popen([str(SHELL), "clipboard-transaction"], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
    owner.stdin.flush()
    if read_child_line(owner, "no-read ready") != "ready":
        raise RuntimeError("Clipboard transaction was not ready for no-read abort")
    owner.stdin.write("arm\n")
    owner.stdin.flush()
    if read_child_line(owner, "no-read armed") != "armed":
        raise RuntimeError("Clipboard transaction failed to arm for no-read abort")
    owner.stdin.write("abort\n")
    owner.stdin.flush()
    status = read_child_line(owner, "no-read abort result", timeout=5)
    if status != "unread":
        raise RuntimeError(f"No-read dispatch was reported as success: {status!r}")
    for mime, payload in snapshot.items():
        if clipboard_data(env, mime) != payload:
            raise RuntimeError(f"No-read abort did not restore original {mime}")


def clipboard_shift_insert_paste(env, text, output_file, expected_text, snapshot):
    owners = []
    try:
        for args in ([], ["--primary"]):
            owner = subprocess.Popen([str(SHELL), "clipboard-transaction", *args], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
            owners.append(owner)
            owner.stdin.write(json.dumps(text, ensure_ascii=False) + "\n")
            owner.stdin.flush()
            if read_child_line(owner, "Shift+Insert readiness") != "ready":
                raise RuntimeError("Clipboard transaction was not ready for Shift+Insert")
        for owner in owners:
            owner.stdin.write("arm\n")
            owner.stdin.flush()
            if read_child_line(owner, "Shift+Insert armed") != "armed":
                raise RuntimeError("Clipboard transaction failed to arm Shift+Insert")
        pasted = subprocess.run([str(HELPER), "--portal", "--shift-insert",
            "--restore-token", "private-e2e-token"], env=env, capture_output=True, timeout=30)
        if pasted.returncode != 0:
            for owner in owners:
                owner.stdin.write("abort\n")
                owner.stdin.flush()
            raise RuntimeError(f"Shift+Insert helper failed: {pasted.stderr!r}")
        for owner in owners:
            owner.stdin.write("sent\n")
            owner.stdin.flush()
        statuses = [read_child_line(owner, "Shift+Insert transfer completion", timeout=12)
                    for owner in owners]
        if statuses[0] != "restored" or statuses[1] not in ("restored", "unread"):
            raise RuntimeError(f"Shift+Insert transactions ended as {statuses!r}")
        wait_for(lambda: output_file.exists() and output_file.read_text(encoding="utf-8") == expected_text,
                 "Shift+Insert pasted exact text", timeout=8)
        for mime, payload in snapshot.items():
            if clipboard_data(env, mime) != payload:
                raise RuntimeError(f"Shift+Insert did not preserve clipboard MIME {mime}")
        if clipboard_data(env, "text/plain", primary=True) != "primary sentinel Ω".encode("utf-8"):
            raise RuntimeError("Shift+Insert transaction did not restore the original PRIMARY selection")
    except Exception:
        for owner in owners:
            if owner.poll() is None:
                try:
                    owner.stdin.write("abort\n")
                    owner.stdin.flush()
                except (BrokenPipeError, OSError):
                    pass
        raise


def authorize_private_kwin_client(env, binary, desktop_name):
    """Grant only the test client fake-input access in this private XDG home."""
    applications = Path(env["XDG_DATA_HOME"]) / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    desktop = applications / (desktop_name + ".desktop")
    desktop.write_text(
        "[Desktop Entry]\n"
        "Type=Application\n"
        f"Name={desktop_name}\n"
        f"Exec={Path(binary).resolve()}\n"
        "X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input;\n",
        encoding="utf-8",
    )


def run_private_session(root, cache_only=False):
    require_commands(("kwin_wayland", "wayland-scanner", "pkg-config", "cc", "dbus-run-session", "wl-paste"))
    if not HELPER.is_file() or not os.access(HELPER, os.X_OK):
        raise RuntimeError(f"Build the production helper first: missing executable {HELPER}")
    build_dir = root / "build"
    build_dir.mkdir()
    injector = compile_injector(build_dir)
    target_binary = compile_target(build_dir)
    slow_provider_binary = compile_slow_clipboard_provider(build_dir)
    env = isolated_env(root, clear_bus=False)
    runtime = Path(env["XDG_RUNTIME_DIR"])
    authorize_private_kwin_client(env, injector, "simple-stt-unicode-test-injector")
    authorize_private_kwin_client(env, HELPER, "simple-stt-unicode-test-helper")
    if shutil.which("kbuildsycoca6"):
        checked(["kbuildsycoca6", "--noincremental"], env=env, timeout=20,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    logs = root / "logs"
    logs.mkdir()
    kwin_log = (logs / "kwin.log").open("wb")
    portal_log = (logs / "portal.log").open("wb")
    target_log = (logs / "target.log").open("wb")
    kwin = portal = target = stress_owner = None
    atspi_launcher = atspi_registry = None
    try:
        kwin_env = env.copy()
        kwin_env.pop("QT_QPA_PLATFORM", None)
        kwin_env.pop("GDK_BACKEND", None)
        # KWin 6.7 filters the private fake-input global through app desktop
        # metadata; the portal mock's injected client is a disposable test
        # binary. Disable that policy only inside this private compositor.
        kwin_env["KWIN_WAYLAND_NO_PERMISSION_CHECKS"] = "1"
        kwin = subprocess.Popen([
            "kwin_wayland", "--virtual", "--socket", env["WAYLAND_DISPLAY"],
            "--width", "900", "--height", "600", "--no-global-shortcuts",
            "--no-kactivities", "--no-lockscreen",
        ], env=kwin_env, stdin=subprocess.DEVNULL, stdout=kwin_log, stderr=subprocess.STDOUT,
           start_new_session=True)
        wayland_socket = runtime / env["WAYLAND_DISPLAY"]
        wait_for(lambda: wayland_socket.exists() or kwin.poll() is not None,
                 "private virtual KWin Wayland socket")
        if kwin.poll() is not None:
            raise RuntimeError(f"Private KWin exited during startup; see {logs / 'kwin.log'}")



        portal = subprocess.Popen([sys.executable, str(FIXTURES / "portal_mock.py"), str(injector)],
            env=env, stdin=subprocess.DEVNULL, stdout=portal_log, stderr=subprocess.STDOUT,
            start_new_session=True)
        wait_for(lambda: b"READY" in (logs / "portal.log").read_bytes() or portal.poll() is not None,
                 "private D-Bus portal mock")
        if portal.poll() is not None:
            raise RuntimeError(f"Private portal mock exited; see {logs / 'portal.log'}")

        atspi_launcher, atspi_registry = start_private_accessibility_bus(env)

        ready = root / "target.ready"
        result_file = root / "target.txt"
        target_env = env.copy()
        # These cases test portal input and clipboard bytes. Fedora GTK3 can
        # crash in its accessibility key-snooper list with the private registry.
        # Accessibility insertion has its own suite; disable only this fixture
        # bridge, never the real helper or the user desktop.
        target_env["NO_AT_BRIDGE"] = "1"
        target = subprocess.Popen([str(target_binary), str(ready), str(result_file)], env=target_env,
            stdin=subprocess.DEVNULL, stdout=target_log, stderr=subprocess.STDOUT,
            start_new_session=True)
        wait_for(lambda: ready.exists() or target.poll() is not None, "real GTK editable window")
        if target.poll() is not None:
            raise RuntimeError(f"GTK target failed to start; see {logs / 'target.log'}")


        protocol_probe = subprocess.run([str(injector), "--probe"], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
        if protocol_probe.returncode != 0:
            details = ""
            if shutil.which("wayland-info"):
                info = subprocess.run(["wayland-info"], env=env, stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT, text=True, timeout=5)
                details = "\n" + "\n".join(line for line in info.stdout.splitlines()
                    if "fake_input" in line or "fake-input" in line)
            raise RuntimeError("Private KWin fake-input protocol unavailable: "
                               f"probe exit={protocol_probe.returncode};{details}")
        # Verify a multi-MIME source owned by the test editor before paste.
        initial_text = "before\n"
        mime_payloads = {
            "application/x-simple-stt-e2e": b"\x00\x01Simple-STT clipboard fixture\xff",
            "image/png": bytes.fromhex("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000b49444154789c636000020000050001a5f645400000000049454e44ae426082"),
        }
        wait_for(lambda: result_file.exists(), "target initialized clipboard")
        for mime, payload in mime_payloads.items():
            try:
                actual = clipboard_data(env, mime)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
                raise RuntimeError(f"Private compositor did not offer expected clipboard MIME {mime}: {exc}")
            if actual != payload:
                raise RuntimeError(f"Initial clipboard payload mismatch for {mime}")
        original_text = clipboard_data(env, "text/plain;charset=utf-8")
        if original_text != "original clipboard Ω".encode():
            raise RuntimeError("Initial clipboard text was not the fixture value")

        clipboard_snapshot = {**mime_payloads, "text/plain;charset=utf-8": original_text}
        if cache_only:
            clipboard_cache_regression(env, root, result_file, slow_provider_binary, injector)
            return
        initial_text = result_file.read_text(encoding="utf-8")
        stress_snapshot = stress_clipboard_snapshot()
        stress_owner = clipboard_slow_offer_regression(
            env, root, result_file, initial_text, slow_provider_binary, stress_snapshot, injector)
        clipboard_snapshot = stress_snapshot
        initial_text = result_file.read_text(encoding="utf-8")
        run_retry_cli(env, root, result_file, initial_text, clipboard_snapshot, injector)
        retry_expected = initial_text + "retry\nمرحبا 🙂"
        gtk_payload = "GTK paste café\nمرحبا 🙂\nline two"
        gtk_expected = retry_expected + gtk_payload
        gtk_owner = clipboard_transaction_paste(env, gtk_payload, result_file, gtk_expected,
                                                clipboard_snapshot)
        primary_sentinel = "primary sentinel Ω".encode("utf-8")
        subprocess.run(["wl-copy", "--primary", "--type", "text/plain"],
            input=primary_sentinel, env=env, check=True, timeout=8)
        wait_for(lambda: clipboard_data(env, "text/plain", primary=True) == primary_sentinel,
                 "private PRIMARY selection setup", timeout=5)
        shift_payload = "Shift+Insert مرحبا 🙂"
        shift_expected = gtk_expected + shift_payload
        clipboard_shift_insert_paste(env, shift_payload, result_file, shift_expected,
                                     clipboard_snapshot)
        if shutil.which("kitty"):
            # Kitty prompts before pasting multiline text by default; this
            # probe checks the configured Ctrl+Shift+V path without UI consent.
            kitty_expected = "Kitty paste café مرحبا 🙂"
            kitty_ready, kitty_output = root / "kitty-paste.ready", root / "kitty-paste.txt"
            kitty_log = (root / "kitty-paste-debug.log").open("wb")
            kitty_env = env.copy()
            kitty_env["WAYLAND_DEBUG"] = "client"
            kitty = subprocess.Popen([
                shutil.which("kitty"), "--config", "NONE", "--debug-keyboard", "--title", "Simple STT Kitty Paste E2E",
                sys.executable, str(FIXTURES / "tty_target.py"), str(kitty_ready), str(kitty_output),
                str(len(kitty_expected.encode("utf-8"))),
            ], env=kitty_env, stdin=subprocess.DEVNULL, stdout=kitty_log,
               stderr=subprocess.STDOUT, start_new_session=True)
            try:
                wait_for(lambda: kitty_ready.exists() or kitty.poll() is not None,
                         "Kitty raw terminal paste target", timeout=12)
                if kitty.poll() is not None:
                    raise RuntimeError("Kitty exited before paste target became ready")
                checked([str(injector), "--click", "450", "300"], env=env, timeout=8)
                kitty_owner = clipboard_transaction_paste(env, kitty_expected, kitty_output,
                    kitty_expected, clipboard_snapshot, terminal=True, dispatch_delay=0.5)
            finally:
                stop_process(kitty)
                kitty_log.close()
            kitty_result = "; Kitty Ctrl+Shift+V paste passed"
        else:
            kitty_result = "; Kitty unavailable, skipped"
        clipboard_no_read_abort(env, "must not report pasted", clipboard_snapshot)
        clipboard_delayed_read(env, "delayed paste 🌙", clipboard_snapshot)
        clipboard_newer_copy_race(env, "stale transaction text")
        clipboard_after_read_copy_race(env, "settling race transcript")
        empty_clipboard_payload = "paste with initially empty clipboard Ω"
        clipboard_empty_selection_paste(env, empty_clipboard_payload, result_file,
            result_file.read_text(encoding="utf-8") + empty_clipboard_payload)

        slow_clipboard_result = "; slow clipboard regression passed (37 image formats, 3 serialized 2s EPS aliases, exact restore, stalled-format timeout before selection change)"
        print("PASS: GTK multiline, Shift+Insert, Rust retry, and Kitty mixed-Unicode paste passed; clipboard formats "
              "were restored, delayed reads waited, no-read abort stayed unread, newer copies were preserved, and an empty clipboard was restored" + kitty_result + slow_clipboard_result)
    finally:
        stop_process(target)
        stop_process(stress_owner)
        stop_process(portal)
        stop_process(atspi_registry)
        stop_process(atspi_launcher)
        stop_process(kwin)
        for stream in (kwin_log, portal_log, target_log):
            stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inside-session", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--root", help=argparse.SUPPRESS)
    parser.add_argument("--cache-only", action="store_true",
                        help="Test the resident current-clipboard buffer on a private compositor")
    args = parser.parse_args()
    if args.inside_session:
        run_private_session(Path(args.root), cache_only=args.cache_only)
        return
    require_commands(("dbus-run-session",))
    root = Path(tempfile.mkdtemp(prefix="simple-stt-unicode-e2e-"))
    env = isolated_env(root)
    # Strip all host display, bus and clipboard-adjacent session variables
    # before dbus-run-session creates a completely private session bus.
    session_log = root / "session.log"
    log_stream = session_log.open("w", encoding="utf-8")
    session = subprocess.run(["dbus-run-session", "--", sys.executable,
        str(Path(__file__).resolve()), "--inside-session", "--root", str(root),
        *(["--cache-only"] if args.cache_only else [])],
        env=env, timeout=180, stdout=log_stream, stderr=subprocess.STDOUT, text=True)
    log_stream.close()
    session_output = session_log.read_text(encoding="utf-8")
    if session.returncode != 0:
        if session_output:
            print(session_output, file=sys.stderr, end="" if session_output.endswith("\n") else "\n")
        print(f"Private session evidence retained at {root}", file=sys.stderr)
        raise subprocess.CalledProcessError(session.returncode, session.args, session_output)
    for line in session_output.splitlines():
        if line.startswith("PASS:"):
            print(line)
    shutil.rmtree(root)

if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
