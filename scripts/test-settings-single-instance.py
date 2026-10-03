#!/usr/bin/env python3
"""Real-process Settings singleton, shared Close, and crash recovery regression."""
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug' / ('simple-stt-settings.exe' if os.name == 'nt' else 'simple-stt-settings')


def main():
    processes = []
    with tempfile.TemporaryDirectory(prefix='simple-stt-settings-singleton-') as temporary:
        directory = Path(temporary)
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(directory / 'config.json'),
                   SIMPLE_STT_RUNTIME_ROOT=str(directory / 'runtime'),
                   XDG_DATA_HOME=str(directory / 'data'), LOCALAPPDATA=str(directory / 'data'))

        def launch(environment=env):
            output = directory / f'output-{len(processes)}.txt'
            errors = directory / f'errors-{len(processes)}.txt'
            with output.open('w') as stdout, errors.open('w') as stderr:
                process = subprocess.Popen([str(BINARY), '--no-browser'], cwd=ROOT,
                                           env=environment, stdout=stdout, stderr=stderr)
            processes.append(process)
            return process, output, errors

        def url_for(start):
            process, output, errors = start
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                url = output.read_text().strip()
                if url:
                    assert url.startswith('http://127.0.0.1:') and '#token=' in url
                    return url
                assert process.poll() is None, errors.read_text()
                time.sleep(0.02)
            raise AssertionError('Settings did not publish a URL')

        def api(url, path, body=None, authenticated=True):
            parsed = urlparse(url)
            origin = f'{parsed.scheme}://{parsed.netloc}'
            request = Request(origin + path, data=None if body is None else json.dumps(body).encode())
            request.add_header('Origin', origin)
            if authenticated:
                request.add_header('X-Simple-STT-Token', parse_qs(parsed.fragment)['token'][0])
            if body is not None:
                request.add_header('Content-Type', 'application/json')
            with urlopen(request, timeout=3) as response:
                return json.load(response)

        def owner_for(url):
            pid = api(url, '/api/health')['pid']
            return next(process for process in processes if process.pid == pid)

        try:
            starts = [launch() for _ in range(12)]
            urls = [url_for(start) for start in starts]
            assert len(set(urls)) == 1, 'concurrent launches created different servers'
            url = urls[0]
            owner = owner_for(url)
            for process, _, errors in starts:
                if process is not owner:
                    assert process.wait(timeout=3) == 0, errors.read_text()
            assert sum(process.poll() is None for process in processes) == 1
            print('PASS: 12 concurrent Settings launches share one server')

            reopened = launch()
            assert url_for(reopened) == url
            assert reopened[0].wait(timeout=3) == 0
            try:
                api(url, '/api/close', {}, authenticated=False)
                raise AssertionError('unauthenticated Close succeeded')
            except HTTPError as error:
                assert error.code == 400
            assert owner.poll() is None
            assert api(url, '/api/close', {})['ok'] is True
            # Launch while the old server may still be exiting.
            fresh = launch()
            fresh_url = url_for(fresh)
            assert fresh_url != url
            assert owner.wait(timeout=3) == 0
            assert owner_for(fresh_url) is fresh[0]
            print('PASS: reopened UI closes the shared server; immediate reopen starts one new server')

            fresh[0].kill()
            fresh[0].wait(timeout=3)
            recovered = launch()
            recovered_url = url_for(recovered)
            assert recovered_url != fresh_url
            assert owner_for(recovered_url) is recovered[0]
            print('PASS: crashed server releases its lock; stale session recovers')

            independent = launch(dict(env, SIMPLE_STT_RUNTIME_ROOT=str(directory / 'other-runtime')))
            independent_url = url_for(independent)
            assert independent_url != recovered_url
            assert owner_for(independent_url) is independent[0]
            assert api(independent_url, '/api/close', {})['ok'] is True
            assert independent[0].wait(timeout=3) == 0
            assert api(recovered_url, '/api/health')['ok'] is True
            print('PASS: independent installations retain separate Settings sessions')

            if os.name != 'nt':
                os.kill(recovered[0].pid, signal.SIGSTOP)
                try:
                    blocked = launch()
                    assert blocked[0].wait(timeout=15) == 1
                    assert blocked[1].read_text() == '', 'unresponsive server spawned a duplicate'
                    assert 'no second server was started' in blocked[2].read_text()
                finally:
                    os.kill(recovered[0].pid, signal.SIGCONT)
                assert api(recovered_url, '/api/health')['ok'] is True
                print('PASS: unresponsive existing server cannot spawn a duplicate')
            assert api(recovered_url, '/api/close', {})['ok'] is True
            assert recovered[0].wait(timeout=3) == 0
            assert not list((directory / 'data').rglob('settings-session.json')), 'Close left session metadata'
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)


if __name__ == '__main__':
    main()
