#!/usr/bin/env python3
"""Real clean delivery checks; requires the disposable display from test-linux-paste.py."""
import os
import ast
import json
import socket
import threading
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def wait_for(predicate, message):
    deadline = time.monotonic() + 6
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.025)
    raise AssertionError(message)


def main():
    assert os.environ.get("SIMPLE_STT_PASTE_X11_E2E") == "1", "Use isolated test-linux-paste.py"
    launcher = subprocess.Popen(["/usr/libexec/at-spi-bus-launcher", "--launch-immediately"],
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    registry = None
    try:
        def bus_address():
            proc = subprocess.run(["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus",
                                   "--method", "org.a11y.Bus.GetAddress"], capture_output=True, text=True, timeout=2)
            return ast.literal_eval(proc.stdout)[0] if proc.returncode == 0 else None
        address = None
        for _ in range(30):
            address = bus_address()
            if address: break
            time.sleep(.05)
        assert address, "isolated accessibility bus did not start"
        os.environ["AT_SPI_BUS_ADDRESS"] = address
        registry = subprocess.Popen(["/usr/libexec/at-spi2-registryd"],
                                    env=dict(os.environ, DBUS_SESSION_BUS_ADDRESS=address),
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        time.sleep(.3)
        run_checks()
    finally:
        if registry: registry.terminate(); registry.communicate(timeout=5)
        launcher.terminate(); launcher.communicate(timeout=5)


def run_checks():
    with tempfile.TemporaryDirectory(prefix="simple-stt-clean-") as folder:
        folder = Path(folder)
        ready, output = folder / "ready", folder / "output"
        target = folder / "target.py"
        target.write_text('''import gi,sys,pathlib
gi.require_version('Gtk','3.0')
from gi.repository import Gtk,GLib
win=Gtk.Window(title='Simple STT isolated accessible target')
edit=Gtk.Entry();edit.set_text('before ');win.add(edit)
other=Gtk.Window(title='Simple STT isolated other target')
other_edit=Gtk.Entry();other.add(other_edit)
other_edit.connect('changed',lambda *args:pathlib.Path(sys.argv[3]).write_text(other_edit.get_text()))
other.show_all()
def changed(*args): pathlib.Path(sys.argv[2]).write_text(edit.get_text())
edit.connect('changed',changed)
win.show_all();win.present();edit.grab_focus();edit.set_position(-1)
def ready(): pathlib.Path(sys.argv[1]).write_text('ready');return False
GLib.timeout_add(1000,ready);Gtk.main()
''')
        env = dict(os.environ, NO_AT_BRIDGE="0", GTK_A11Y="always")
        other_output = folder / 'other-output'
        app = subprocess.Popen(["/usr/bin/python3", str(target), str(ready), str(output), str(other_output)], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        old = subprocess.Popen(["xclip", "-quiet", "-selection", "clipboard", "-target", "application/x-simple-stt-test"],
                               stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sentinel = b"original-binary\x00format"
        old.stdin.write(sentinel); old.stdin.close()
        try:
            wait_for(ready.exists, "accessible editor did not start")
            win = subprocess.check_output(["xdotool", "search", "--name", "Simple STT isolated accessible target"], text=True).splitlines()[0]
            subprocess.run(["xdotool", "windowfocus", "--sync", win], check=True)
            time.sleep(.15)
            payload = "hello مرحبا 🙂 "
            inserted = subprocess.run([str(ROOT / "resources/bin/linux-fast-paste"), "--insert-text"], input=payload.encode(),
                                      capture_output=True, timeout=8, env=env)
            assert inserted.returncode == 0, (inserted.returncode, inserted.stdout, inserted.stderr)
            assert inserted.stdout == b"attempt\n"
            wait_for(lambda: output.exists() and output.read_text() == "before " + payload, "Unicode direct insertion mismatch")
            actual = subprocess.check_output(["xclip", "-selection", "clipboard", "-out", "-target", "application/x-simple-stt-test"], timeout=3)
            assert actual == sentinel, "direct insertion changed non-text clipboard"
            print("PASS direct Unicode insertion leaves binary clipboard untouched")
            for delay in (26, 14, 0):
                # Only this disposable X server receives input. The non-text
                # clipboard owner remains unchanged through both typing modes.
                subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+End'], env=env, check=True)
                before = output.read_text()
                typed = 'Hello مرحبا 🙂 '
                started = time.monotonic()
                result = subprocess.run([str(ROOT / 'resources/bin/linux-fast-paste'), '--type-text',
                                         '--typing-delay-ms', str(delay)], input=typed.encode(),
                                        capture_output=True, timeout=8, env=env)
                elapsed = time.monotonic() - started
                assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
                assert result.stdout == b'attempt\n'
                wait_for(lambda: output.read_text() == before + typed, 'Unicode typing mismatch')
                assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-out', '-target',
                                                'application/x-simple-stt-test'], timeout=3) == sentinel
                if delay:
                    assert elapsed >= (len(typed) - 1) * delay / 1000, 'paced typing skipped delays'
                print(f'PASS Unicode typing delay={delay} elapsed_ms={elapsed*1000:.1f}, clipboard untouched')
            for cause in ('focus', 'abort'):
                subprocess.run(['xdotool', 'windowfocus', '--sync', win], env=env, check=True)
                subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+End'], env=env, check=True)
                before = output.read_text()
                pending = subprocess.Popen([str(ROOT / 'resources/bin/linux-fast-paste'), '--type-text',
                                            '--typing-delay-ms', '100'], env=env, stdin=subprocess.PIPE,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    pending.stdin.write(b'Controlled typing must stop here.');pending.stdin.close();pending.stdin=None
                    wait_for(lambda: output.read_text() != before, 'paced typing did not start')
                    if cause == 'focus':
                        other_win = subprocess.check_output(['xdotool', 'search', '--name', 'Simple STT isolated other target'], env=env, text=True).splitlines()[0]
                        subprocess.run(['xdotool', 'windowfocus', '--sync', other_win], env=env, check=True)
                        stdout, stderr = pending.communicate(timeout=3)
                        assert pending.returncode == 2, (pending.returncode, stdout, stderr)
                        assert not other_output.exists() or not other_output.read_text(), 'typing redirected to another window'
                    else:
                        pending.terminate();pending.communicate(timeout=3)
                    time.sleep(.075)
                    stopped = output.read_text();time.sleep(.2)
                    assert output.read_text() == stopped and len(stopped) < len(before) + 32, 'typing continued after cancellation/focus loss'
                    assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-out', '-target', 'application/x-simple-stt-test'], timeout=3) == sentinel
                    print(f'PASS paced Unicode typing stops after {cause}; no clipboard change or redirected text')
                finally:
                    if pending.poll() is None:pending.kill();pending.communicate(timeout=3)
            subprocess.run(['xdotool', 'windowfocus', '--sync', win], env=env, check=True)
            subprocess.run(['xdotool', 'key', '--clearmodifiers', 'ctrl+End'], env=env, check=True)
            test_retry(folder, output, output.read_text())
            transcript_file = os.environ.get("SIMPLE_STT_TEST_TRANSCRIPT_FILE")
            if transcript_file:
                transcript = " ".join(Path(transcript_file).read_text(encoding="utf-8").splitlines())
                for mode in ("paste_ctrl_v", "type"):
                    case = folder / mode
                    case.mkdir()
                    test_retry(case, output, output.read_text(), mode=mode, payload=transcript)
                    assert output.read_text().endswith(transcript), "ASR final word lost in delivery"
                    print(f"PASS complete generated-audio transcript delivered through {mode}")
        finally:
            app.terminate(); app.communicate(timeout=5)
            old.terminate(); old.wait(timeout=5)
        text = "مرحبا world 🙂 " * 10000
        owner = subprocess.Popen([str(ROOT / "target/debug/simple-stt-linux"), "clipboard-owner"], stdin=subprocess.PIPE,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        owner.stdin.write(text.encode()); owner.stdin.close()
        try:
            wait_for(lambda: subprocess.run(["xclip", "-selection", "clipboard", "-out", "-target", "x-kde-passwordManagerHint"],
                                            capture_output=True, timeout=3).stdout == b"secret", "history hint missing")
            actual = subprocess.check_output(["xclip", "-selection", "clipboard", "-out", "-target", "UTF8_STRING"], timeout=5)
            assert actual == text.encode(), "incremental large Unicode transfer mismatch"
            copied = subprocess.run(["xclip", "-selection", "clipboard"], input=b"new user copy", timeout=3)
            assert copied.returncode == 0
            owner.wait(timeout=3)
            assert subprocess.check_output(["xclip", "-selection", "clipboard", "-out"], timeout=3) == b"new user copy"
            print("PASS sensitive X11 owner offers exclusion hint, full incremental Unicode data and yields to user copy")
        finally:
            if owner.poll() is None: owner.terminate(); owner.wait(timeout=5)
            if owner.stderr: owner.stderr.close()


def test_retry(folder, output, initial, mode="paste_ctrl_v", payload="Retry EXACT! مرحبا 🙂 "):
    """Execute the real CLI against an authenticated fixture, never infer or record."""
    runtime = folder / "runtime"
    runtime.mkdir()
    data = folder / "data"
    config = folder / "config.json"
    config.write_text(json.dumps({"schema_version": 9, "general": {"ui_language": "en"}, "output": {
        "delivery_mode": mode, "linux_automation_backend": "xdotool", "paced_typing_enabled": False,
        "preserve_clipboard": False, "lowercase": True, "remove_punctuation": True, "trailing_space": True}}))
    hashed = 0xcbf29ce484222325
    for byte in str(runtime).encode(): hashed = ((hashed ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    state_root = data / "simple-stt" / "instances" / f"runtime-{hashed:016x}"
    state_dir = state_root / "state"
    state_dir.mkdir(parents=True)
    (state_root / "linux-token").write_text("isolated-retry-token")
    session_file = state_root / "linux-recording-session.json"
    original_session = {"recording": True, "session_id": 42, "updated_at": 1}
    session_file.write_text(json.dumps(original_session))
    commands, errors, notices, phases = [], [], [], []
    deliveries = set()
    cached = [payload]
    cache_failure = [False]
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0)); listener.listen(); listener.settimeout(.1)
    done = threading.Event()
    (state_dir / "linux-capture-state.json").write_text(json.dumps({"protocol": 3, "pid": os.getpid(),
        "address": f"127.0.0.1:{listener.getsockname()[1]}", "started_unix_ms": 0}))
    def serve():
        while not done.is_set():
            try: connection, _ = listener.accept()
            except socket.timeout: continue
            try:
                with connection, connection.makefile("rwb") as stream:
                    hello = json.loads(stream.readline())
                    assert hello["token"] == "isolated-retry-token"
                    stream.write(json.dumps({"type": "hello_ack", "protocol": 3, "service_pid": os.getpid()}).encode() + b"\n"); stream.flush()
                    while line := stream.readline():
                        request = json.loads(line); name = request["command"]["name"]; commands.append(name)
                        assert name in ["last_delivery", "cancel", "begin_delivery", "show_notice", "delivery_progress", "delivery_complete"], f"retry launched work: {name}"
                        if name == "show_notice": notices.append(request["command"]["text"])
                        if name == "begin_delivery": deliveries.add(request["command"]["session_id"])
                        if name == "delivery_progress":
                            assert request["command"]["session_id"] in deliveries, "progress sent without a live delivery"
                            phases.append(request["command"]["phase"])
                        if name == "delivery_complete": deliveries.remove(request["command"]["session_id"])
                        response = {"ok": True, "message": "fixture", "events": [], "values": {}}
                        if name == "last_delivery" and cache_failure[0]: response["ok"] = False
                        if name == "last_delivery" and cached[0] is not None: response["values"]["text"] = cached[0]
                        stream.write(json.dumps({"type": "response", "request_id": request["request_id"], "response": response}).encode() + b"\n"); stream.flush()
            except Exception as error: errors.append(error)
    thread = threading.Thread(target=serve); thread.start()
    env = dict(os.environ, SIMPLE_STT_CONFIG=str(config), SIMPLE_STT_RUNTIME_ROOT=str(runtime), XDG_DATA_HOME=str(data))
    binary = ROOT / "target/debug/simple-stt-linux"
    try:
        # Hold the trigger modifiers on this private X server. Retry must wait
        # instead of injecting Meta+Ctrl+V into the editor or clearing keys.
        subprocess.run(["xdotool", "keydown", "Super_L", "Control_L"], check=True)
        retry = subprocess.Popen([str(binary), "retry-delivery"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            wait_for(lambda: "begin_delivery" in commands, "retry waiting state did not start while keys were held")
            time.sleep(2.25)
            assert retry.poll() is None, "retry did not wait for held modifiers"
            assert output.read_text() == initial, "retry inserted before modifiers were released"
            subprocess.run(["xdotool", "keyup", "Super_L", "Control_L"], check=True)
            _, stderr = retry.communicate(timeout=15)
            assert retry.returncode == 0, stderr
        finally:
            subprocess.run(["xdotool", "keyup", "Super_L", "Control_L"], check=True)
            if retry.poll() is None: retry.kill(); retry.communicate()
        wait_for(lambda: output.read_text() == initial + payload, "retry did not use exact cached text/current target")
        if mode != "type":
            assert subprocess.check_output(["xclip", "-selection", "clipboard", "-out"], timeout=3) == payload.encode(), "option-off did not leave an ordinary copy"
        else:
            assert "typing" in phases and "pasting" not in phases, phases
        offered = subprocess.check_output(["xclip", "-selection", "clipboard", "-out", "-target", "TARGETS"], timeout=3)
        assert b"x-kde-passwordManagerHint" not in offered, "option-off retained history exclusion"
        assert [command for command in commands if command not in ("show_notice", "delivery_progress")] == ["last_delivery", "cancel", "begin_delivery", "delivery_complete"], commands
        assert notices == ["Retry sent"], notices
        assert commands.index("begin_delivery") < commands.index("delivery_progress") < commands.index("delivery_complete"), commands
        cached[0] = None; commands.clear(); notices.clear(); session_file.write_text(json.dumps(original_session))
        result = subprocess.run([str(binary), "retry-delivery"], env=env, capture_output=True, timeout=15)
        assert result.returncode == 0
        assert commands == ["last_delivery", "show_notice"] and json.loads(session_file.read_text()) == original_session, "empty retry cancelled work"
        assert notices == ["No dictation to retry"], notices
        assert output.read_text() == initial + payload, "empty retry inserted something"
        commands.clear(); notices.clear(); cache_failure[0] = True
        result = subprocess.run([str(binary), "retry-delivery"], env=env, capture_output=True, timeout=15)
        assert result.returncode != 0, "cache failure was silently accepted"
        assert commands == ["last_delivery", "show_notice"], commands
        assert notices == ["Retry failed. Try again."], notices
        assert output.read_text() == initial + payload and json.loads(session_file.read_text()) == original_session
        for path in state_root.rglob("*"):
            if path.is_file(): assert payload.encode() not in path.read_bytes(), "retry text was persisted to disk"
        assert not errors, errors
        print("PASS retry waits for modifiers, reports progress/result/empty cache, uses exact cached text and never records or persists it")
    finally:
        done.set(); thread.join(timeout=3); listener.close()


if __name__ == "__main__":
    main()
