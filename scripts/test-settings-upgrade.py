#!/usr/bin/env python3
"""Settings upgrade must open the current build and preserve an older open page."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/simple-stt-settings'


def main():
    if os.name == 'nt':
        print('SKIP atomic running-executable replacement: Linux package-upgrade regression')
        return
    children = []
    with tempfile.TemporaryDirectory(prefix='simple-stt-settings-upgrade-') as temporary:
        private = Path(temporary)
        executable = private / 'simple-stt-settings'
        shutil.copy2(BINARY, executable)
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(private / 'config.json'),
                   SIMPLE_STT_RUNTIME_ROOT=str(private / 'runtime'),
                   XDG_DATA_HOME=str(private / 'data'), XDG_CONFIG_HOME=str(private / 'config'),
                   DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-settings-upgrade-bus',
                   DISPLAY='', WAYLAND_DISPLAY='')

        def launch():
            output = private / f'output-{len(children)}'
            errors = private / f'errors-{len(children)}'
            with output.open('w') as stdout, errors.open('w') as stderr:
                process = subprocess.Popen([str(executable), '--no-browser'], env=env,
                                           stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
            children.append(process)
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                text = output.read_text().strip()
                if text:
                    assert text.startswith('http://127.0.0.1:')
                    return process, text
                assert process.poll() is None, errors.read_text()
                time.sleep(.02)
            raise AssertionError('Settings startup timed out')

        def api(url, path, body=None):
            parsed = urlparse(url)
            origin = f'{parsed.scheme}://{parsed.netloc}'
            request = Request(origin + path, data=None if body is None else json.dumps(body).encode(),
                              headers={'Origin': origin, 'X-Simple-STT-Token': parse_qs(parsed.fragment)['token'][0],
                                       'Content-Type': 'application/json'})
            with urlopen(request, timeout=3) as response:
                return json.load(response)

        try:
            older, old_url = launch()
            api(old_url, '/api/state')
            before = (private / 'config.json').read_bytes() if (private / 'config.json').exists() else None
            # Package managers replace the directory entry while the old process
            # still executes its previous inode. An ELF overlay changes identity
            # without changing behavior or requiring another expensive Rust build.
            replacement = private / 'replacement'
            shutil.copy2(BINARY, replacement)
            with replacement.open('ab') as binary:
                binary.write(b'\0settings-upgrade-fixture\0')
            os.replace(replacement, executable)
            current, current_url = launch()
            assert current_url != old_url, 'upgraded Settings silently reopened the old UI server'
            assert current.poll() is None and older.poll() is None, 'opening a new build discarded the older page'
            assert api(old_url, '/api/health')['pid'] == older.pid
            assert api(current_url, '/api/health')['pid'] == current.pid
            assert api(old_url, '/api/health')['build_id'] != api(current_url, '/api/health')['build_id']
            assert ((private / 'config.json').read_bytes() if (private / 'config.json').exists() else None) == before
            repeat, repeat_url = launch()
            assert repeat_url == current_url and repeat.wait(timeout=3) == 0, 'same build did not reuse its server'
            assert api(current_url, '/api/close', {})['ok']
            assert current.wait(timeout=3) == 0
            assert api(old_url, '/api/health')['ok'], 'closing current Settings also closed the older page'
            assert api(old_url, '/api/close', {})['ok']
            assert older.wait(timeout=3) == 0
            assert not list((private / 'data').rglob('settings-session*.json'))
            print('PASS upgrade opens the current UI; same-build reuse, older-page preservation, config safety and cleanup')
        finally:
            for process in children:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)


if __name__ == '__main__':
    main()
