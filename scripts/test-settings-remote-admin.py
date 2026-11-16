#!/usr/bin/env python3
"""Isolated real-process checks for authenticated remote Settings administration."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug' / ('simple-stt-settings.exe' if os.name == 'nt' else 'simple-stt-settings')


def assert_wildcard_listener(process, port):
    if sys.platform != 'linux':
        return
    for table in (Path(f'/proc/{process.pid}/net/tcp'), Path(f'/proc/{process.pid}/net/tcp6')):
        if not table.exists():
            continue
        for line in table.read_text().splitlines()[1:]:
            columns = line.split()
            address, hex_port = columns[1].split(':')
            if int(hex_port, 16) == port and columns[3] == '0A' and set(address) == {'0'}:
                return
    raise AssertionError('Settings did not bind a wildcard interface')


def request(url, path, *, token=None, origin=None, host=None, body=None):
    parsed = urlparse(url)
    headers = {}
    if token is not None:
        headers['X-Simple-STT-Token'] = token
    if origin is not None:
        headers['Origin'] = origin
    if host is not None:
        headers['Host'] = host
    data = None if body is None else json.dumps(body).encode()
    if body is not None:
        headers['Content-Type'] = 'application/json'
    req = Request(f'{parsed.scheme}://{parsed.netloc}{path}', data=data, headers=headers)
    try:
        with urlopen(req, timeout=3) as response:
            return response.status, response.read()
    except HTTPError as error:
        return error.code, error.read()


def main():
    assert BINARY.is_file(), f'build Settings first: {BINARY}'
    process = local_process = None
    with tempfile.TemporaryDirectory(prefix='simple-stt-settings-remote-') as temporary:
        directory = Path(temporary)
        token = 'remote-test-token-' + ('x' * 40)
        token_file = directory / 'admin.token'
        token_file.write_text(token + '\n')
        output = directory / 'stdout.txt'
        errors = directory / 'stderr.txt'
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        origin = f'http://127.0.0.1:{port}'
        env = dict(
            os.environ,
            SIMPLE_STT_CONFIG=str(directory / 'config.json'),
            SIMPLE_STT_RUNTIME_ROOT=str(directory / 'runtime'),
            XDG_DATA_HOME=str(directory / 'data'),
            LOCALAPPDATA=str(directory / 'data'),
        )
        with output.open('w') as stdout, errors.open('w') as stderr:
            process = subprocess.Popen(
                [str(BINARY), '--listen', f'0.0.0.0:{port}', '--public-url', origin,
                 '--token-file', str(token_file), '--no-browser'],
                cwd=ROOT, env=env, stdout=stdout, stderr=stderr,
            )
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                url = output.read_text().strip()
                if url:
                    break
                assert process.poll() is None, errors.read_text()
                time.sleep(0.05)
            else:
                raise AssertionError('remote Settings did not publish its browser URL')

            assert url == origin + '/', f'unexpected remote URL: {url}'
            assert token not in url and '#token=' not in url, 'remote token appeared in browser URL'
            assert_wildcard_listener(process, port)
            assert request(url, '/')[0] == 200, 'sign-in page must be available before token entry'
            assert request(url, '/api/state')[0] == 400, 'API accepted a missing token'
            assert request(url, '/api/state', token=token, host='attacker.example')[0] == 400
            assert request(url, '/api/state', token=token, origin='http://attacker.example')[0] == 400

            status, raw = request(url, '/api/state', token=token, origin=origin)
            assert status == 200, raw.decode(errors='replace')
            state = json.loads(raw)
            assert state['service_online'] is False, 'headless admin should report missing capture as offline'
            status, raw = request(url, '/api/assets', token=token, origin=origin)
            assert status == 200, raw.decode(errors='replace')
            assert 'assets' in json.loads(raw), 'authenticated asset management API is unavailable'

            # A remote instance and the normal localhost UI must own separate
            # session files even though both use the same executable build.
            local_output = directory / 'local-stdout.txt'
            local_errors = directory / 'local-stderr.txt'
            with local_output.open('w') as stdout, local_errors.open('w') as stderr:
                local_process = subprocess.Popen(
                    [str(BINARY), '--no-browser'], cwd=ROOT, env=env,
                    stdout=stdout, stderr=stderr,
                )
            # A debug build hashes its executable to derive the instance ID;
            # large, uncached binaries can take longer than the remote launch.
            local_deadline = time.monotonic() + 45
            while time.monotonic() < local_deadline:
                local_url = local_output.read_text().strip()
                if local_url:
                    break
                assert local_process.poll() is None, local_errors.read_text()
                time.sleep(0.05)
            else:
                raise AssertionError(
                    'localhost Settings did not publish its URL before the startup deadline; '
                    f'pid={local_process.pid}, exit_code={local_process.poll()}, '
                    f'stdout={local_output.read_text()!r}, stderr={local_errors.read_text()!r}'
                )
            assert '#token=' in local_url
            session_files = list((directory / 'data').rglob('settings-session*.json'))
            assert len(session_files) == 2, 'local and remote instances collided in session storage'

            time.sleep(1)
            assert process.poll() is None, 'remote Settings exited without an explicit Close'
            status, raw = request(url, '/api/close', token=token, origin=origin, body={})
            assert status == 200 and json.loads(raw)['ok'] is True, raw.decode(errors='replace')
            assert process.wait(timeout=5) == 0, errors.read_text()
            session_files = list((directory / 'data').rglob('settings-session*.json'))
            assert len(session_files) == 1, 'remote shutdown removed the localhost session'
            local_token = urlparse(local_url).fragment.removeprefix('token=')
            status, raw = request(local_url, '/api/state', token=local_token)
            assert status == 200, raw.decode(errors='replace')
            status, raw = request(local_url, '/api/close', token=local_token, body={})
            assert status == 200 and json.loads(raw)['ok'] is True
            assert local_process.wait(timeout=5) == 0, local_errors.read_text()
            assert not list((directory / 'data').rglob('settings-session*.json'))
            print('PASS: remote bind, sign-in token auth, Host/Origin validation, headless capture, persistence, and clean shutdown')
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            if local_process is not None and local_process.poll() is None:
                local_process.kill()
                local_process.wait(timeout=5)


if __name__ == '__main__':
    main()
