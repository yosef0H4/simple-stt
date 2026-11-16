#!/usr/bin/python3
"""Live KDE Wayland default-delivery E2E in an owned GTK editor.

May show the desktop's keyboard-control permission dialog. Uses isolated app
settings and a mock transcript cache, but real Rust delivery, clipboard
preservation, RemoteDesktop authorization and compositor keyboard injection.
"""
import importlib.util
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import traceback

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--typing', action='store_true', help='Test selectable Type mode and unchanged clipboard')
args = parser.parse_args()
spec = importlib.util.spec_from_file_location('desktop_fixture', ROOT / 'scripts/test-read-along-desktop.py')
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)
Gtk, GLib = desktop.Gtk, desktop.GLib
folder = Path(os.environ.get('SIMPLE_STT_PORTAL_TEST_DIR',
    str(Path(tempfile.gettempdir()) / f'simple-stt-portal-e2e-{os.getuid()}')))
folder.mkdir(mode=0o700, parents=True, exist_ok=True)
window = Gtk.Window(title=desktop.TITLE)
window.set_default_size(700, 300)
view = Gtk.TextView()
window.add(view)
buffer = view.get_buffer()
window.connect('destroy', Gtk.main_quit)
result = []
keys = []
view.connect('key-press-event', lambda _widget, event: keys.append((event.keyval, int(event.state))) or False)
done = threading.Event()
payload = 'Portal paste: English, مرحبا بالعالم, Ελληνικά, 日本語 🙂\nSecond line.'


def contents():
    return buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), False)


def run():
    listener = socket.socket()
    errors = []
    try:
        desktop.wait_for(lambda: desktop.query_target(folder).get('active'), 10, 'owned editor focus')
        runtime = folder / 'runtime'
        runtime.mkdir(exist_ok=True)
        data = folder / 'data'
        config = folder / 'config.json'
        # Deliberately omit output: exercise the shipped defaults, not a forced
        # portal backend or delivery mode hidden in the test configuration.
        settings = {'schema_version': 9, 'general': {'ui_language': 'en'}}
        if args.typing:
            settings['output'] = {'delivery_mode': 'type'}
        config.write_text(json.dumps(settings))
        hashed = 0xcbf29ce484222325
        for byte in str(runtime).encode():
            hashed = ((hashed ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
        state_root = data / 'simple-stt' / 'instances' / f'runtime-{hashed:016x}'
        state_dir = state_root / 'state'
        state_dir.mkdir(parents=True, exist_ok=True)
        (state_root / 'linux-token').write_text('isolated-portal-test')
        (state_root / 'linux-recording-session.json').write_text(json.dumps({'recording': False, 'session_id': 42, 'updated_at': 1}))
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(.1)
        (state_dir / 'linux-capture-state.json').write_text(json.dumps({'protocol': 3, 'pid': os.getpid(),
            'address': f'127.0.0.1:{listener.getsockname()[1]}', 'started_unix_ms': 0}))

        def serve():
            while not done.is_set():
                try:
                    connection, _ = listener.accept()
                except socket.timeout:
                    continue
                try:
                    with connection, connection.makefile('rwb') as stream:
                        hello = json.loads(stream.readline())
                        assert hello['token'] == 'isolated-portal-test'
                        stream.write(json.dumps({'type': 'hello_ack', 'protocol': 3, 'service_pid': os.getpid()}).encode() + b'\n')
                        stream.flush()
                        while line := stream.readline():
                            request = json.loads(line)
                            name = request['command']['name']
                            assert name in {'last_delivery', 'cancel', 'begin_delivery', 'show_notice', 'delivery_progress', 'delivery_complete'}, name
                            values = {'text': payload} if name == 'last_delivery' else {}
                            response = {'ok': True, 'message': 'fixture', 'events': [], 'values': values}
                            stream.write(json.dumps({'type': 'response', 'request_id': request['request_id'], 'response': response}).encode() + b'\n')
                            stream.flush()
                except Exception as error:
                    errors.append(error)

        server = threading.Thread(target=serve)
        server.start()
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(config), SIMPLE_STT_RUNTIME_ROOT=str(runtime), XDG_DATA_HOME=str(data))
        def clipboard_snapshot():
            formats = subprocess.check_output(['wl-paste', '--list-types'], timeout=5).decode().splitlines()
            return {mime: subprocess.check_output(['wl-paste', '--no-newline', '--type', mime], timeout=5)
                    for mime in formats}
        before = clipboard_snapshot() if args.typing else None
        # Two deliveries also exercise persistence and rotation of restore tokens.
        for number in (1, 2):
            assert desktop.query_target(folder)['active'], 'owned editor lost focus before delivery'
            proc = subprocess.run([str(ROOT / 'target/debug/simple-stt-linux'), 'retry-delivery'],
                env=env, capture_output=True, text=True, timeout=75)
            (folder / f'delivery-{number}.log').write_text(proc.stderr)
            assert proc.returncode == 0, proc.stderr
            if not args.typing:
                assert 'paste native portal=true status=exit status: 0' in proc.stderr, proc.stderr
            desktop.wait_for(lambda: contents() == payload * number, 5, 'exact Unicode insertion')
            if args.typing:
                assert clipboard_snapshot() == before, 'Type changed clipboard formats or data'
        assert not errors, errors
        print('PASS: ' + ('Type, exact multilingual text, unchanged clipboard formats and data, repeated delivery'
            if args.typing else 'default Smart Paste, real RemoteDesktop input, exact multilingual text, repeated delivery'), flush=True)
        result.append(True)
    except Exception:
        print('Editor after failure:', repr(contents()), 'keys:', keys, 'focus:', desktop.query_target(folder), flush=True)
        traceback.print_exc()
    finally:
        done.set()
        if 'server' in locals():
            server.join(timeout=3)
        listener.close()
        GLib.idle_add(Gtk.main_quit)


window.show_all()
view.grab_focus()
window.present()
GLib.timeout_add(600, lambda: (threading.Thread(target=run).start(), False)[1])
Gtk.main()
print('Artifacts:', folder)
raise SystemExit(0 if result else 1)
