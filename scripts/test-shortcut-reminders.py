#!/usr/bin/env python3
"""Exercise real CLI/Settings shortcut transport with a private fake capture service.
No microphone, desktop, clipboard, keyboard injection or capture process is used.
"""
import json
import os
from pathlib import Path
import socketserver
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
TOKEN = 'isolated-shortcut-reminder-token'
commands = []


class CaptureFixture(socketserver.StreamRequestHandler):
    def handle(self):
        hello = json.loads(self.rfile.readline())
        assert hello == {'type': 'hello', 'protocol': 3, 'token': TOKEN}
        self.wfile.write(json.dumps({'type': 'hello_ack', 'protocol': 3, 'service_pid': os.getpid()}).encode() + b'\n')
        self.wfile.flush()
        request = json.loads(self.rfile.readline())
        commands.append(request['command']['name'])
        self.wfile.write(json.dumps({'type': 'response', 'request_id': request['request_id'], 'response': {'ok': True, 'message': 'fixture', 'values': {}, 'events': []}}).encode() + b'\n')
        self.wfile.flush()


def main():
    binary = ROOT / 'target/debug'
    with tempfile.TemporaryDirectory(prefix='stt-shortcuts-') as temporary, socketserver.ThreadingTCPServer(('127.0.0.1', 0), CaptureFixture) as fixture:
        threading.Thread(target=fixture.serve_forever, daemon=True).start()
        folder = Path(temporary)
        state = folder / 'capture.json'
        state.write_text(json.dumps({'protocol': 3, 'pid': os.getpid(), 'address': f'127.0.0.1:{fixture.server_address[1]}', 'started_unix_ms': 0}))
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(folder / 'config.json'), SIMPLE_STT_RUNTIME_ROOT=str(folder / 'runtime'), XDG_DATA_HOME=str(folder / 'data'), XDG_CONFIG_HOME=str(folder / 'config'), DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus', WAYLAND_DISPLAY='unavailable-test-display')
        runtime = folder / 'runtime'
        runtime.mkdir()
        (runtime / 'linux-installation').touch()
        data = folder / 'data/simple-stt/instances/installed'
        (data / 'state').mkdir(parents=True)
        (data / 'state/linux-capture-state.json').write_bytes(state.read_bytes())
        (data / 'linux-token').write_text(TOKEN)
        subprocess.run([str(binary / 'simple-stt-linux'), 'show-shortcuts'], env=env, capture_output=True, text=True, check=True)
        for command in ['show-shortcuts', 'refresh-shortcuts']:
            result = subprocess.run([str(binary / 'simple-stt-ctl'), '--state-file', str(state), '--token', TOKEN, command], env=env, capture_output=True, text=True, check=True)
            assert 'status\tok' in result.stdout, result.stdout
        settings = subprocess.Popen([str(binary / 'simple-stt-settings'), '--no-browser', '--state-file', str(state), '--service-token', TOKEN], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            url = urlparse(settings.stdout.readline().strip())
            token = parse_qs(url.fragment)['token'][0]
            headers = {'Origin': f'{url.scheme}://{url.netloc}', 'X-Simple-STT-Token': token, 'Content-Type': 'application/json'}
            request = Request(f'{url.scheme}://{url.netloc}/api/platform-action', data=json.dumps({'action': 'show_shortcuts'}).encode(), headers=headers)
            with urlopen(request, timeout=5) as response:
                assert response.status == 200
            assert commands == ['show_shortcuts', 'show_shortcuts', 'refresh_shortcuts', 'show_shortcuts'], commands
        finally:
            settings.terminate()
            settings.wait(timeout=5)
            fixture.shutdown()
    print('PASS real shortcut CLI and Settings transport; no recording, cancellation or delivery commands')


if __name__ == '__main__':
    main()
