#!/usr/bin/env python3
"""Check the native Settings lifecycle under Xvfb with a private profile and bus.

Run with xvfb-run -a dbus-run-session -- python3 scripts/test-settings-desktop-linux.py
Pass a path to the launcher to test another installation or an isolated build.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path


def main():
    default_launcher = '/usr/lib/simple-stt/simple-stt-settings'
    launcher = Path(sys.argv[1] if len(sys.argv) > 1 else default_launcher).resolve()
    if not os.environ.get('DISPLAY'):
        raise SystemExit('Run this test on a private Xvfb display')

    with tempfile.TemporaryDirectory(prefix='simple-stt-native-profile-') as temp:
        root = Path(temp)
        env = os.environ.copy()
        for key in ('SIMPLE_STT_CONFIG', 'SIMPLE_STT_SETTINGS_DEV_ORIGIN',
                    'SIMPLE_STT_RUNTIME_ROOT', 'WAYLAND_DISPLAY'):
            env.pop(key, None)
        for key, directory in [('HOME', 'home'), ('XDG_DATA_HOME', 'data'),
                               ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                               ('XDG_RUNTIME_DIR', 'run')]:
            path = root / directory
            path.mkdir(mode=0o700)
            env[key] = str(path)
        env.update(XDG_SESSION_TYPE='x11', GDK_BACKEND='x11', XDG_CURRENT_DESKTOP='KDE')

        def wait_for(check, what, seconds=20):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                if value := check():
                    return value
                time.sleep(0.08)
            raise AssertionError(f'timed out waiting for {what}')

        def run(*args):
            return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=5)

        def start(log):
            with log.open('wb') as file:
                return subprocess.Popen([str(launcher)], env=env, stdout=file, stderr=subprocess.STDOUT)

        def session():
            paths = list((root / 'data').glob('**/settings-session-desktop.json'))
            return json.loads(paths[0].read_text()) if paths else None

        def window_id(pid):
            try:
                windows = run('xdotool', 'search', '--onlyvisible', '--all', '--pid', str(pid),
                              '--name', '^Simple STT Settings$').splitlines()
                return windows[0] if windows else None
            except subprocess.CalledProcessError:
                return None

        def health(info):
            try:
                request = urllib.request.Request(info['origin'] + '/api/health', headers={
                    'Origin': info['origin'],
                    'X-Simple-STT-Token': info['token'],
                })
                with urllib.request.urlopen(request, timeout=0.5) as response:
                    return json.load(response)
            except OSError:
                return None

        for launch in range(2):
            log = root / f'window-{launch}.log'
            proc = start(log)
            try:
                info = wait_for(session, 'native Settings session')
                assert info['pid'] == proc.pid, 'launcher and local Settings API should share one PID on Linux'
                state = wait_for(lambda: health(info), 'authenticated Settings API')
                assert state['ok'] and state['pid'] == proc.pid
                win = wait_for(lambda: window_id(proc.pid), 'visible native Settings window')
                other = start(root / 'reopen.log')
                try:
                    assert other.wait(timeout=10) == 0, (root / 'reopen.log').read_text()
                finally:
                    if other.poll() is None:
                        other.kill()
                        other.wait()
                assert session()['pid'] == proc.pid and window_id(proc.pid) == win
                width = int(re.search(r'^\s*Width:\s+(\d+)', run('xwininfo', '-id', win), re.M)[1])

                # Use the same close action a user clicks on the custom title bar.
                def click_close():
                    if proc.poll() is not None:
                        return True
                    if window_id(proc.pid) is not None:
                        click = subprocess.run(
                            ['xdotool', 'mousemove', '--window', win, str(width - 16), '15',
                             'click', '1'], capture_output=True, text=True, timeout=5,
                        )
                        # GTK can destroy the window while WebKit is still shutting down.
                        if click.returncode and window_id(proc.pid) is not None:
                            raise AssertionError(click.stderr)
                    return False

                wait_for(click_close, 'window and server to exit from title-bar Close', 8)
                assert proc.returncode == 0, 'native Settings failed during shutdown'
                assert not session(), 'native Settings session file was not removed'
                assert not health(info), 'Settings API still answered after closing the window'
                print(f'PASS native Settings launch {launch+1}, single instance, title-bar close, and API shutdown')
            except BaseException:
                print(log.read_text(errors='replace')[-3000:], file=sys.stderr)
                raise
            finally:
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait()


if __name__ == "__main__":
    main()
