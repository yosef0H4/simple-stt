#!/usr/bin/env python3
"""Installed-package smoke check; no microphone, desktop, or clipboard access."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import threading
import time
import wave
from urllib.parse import parse_qs, urlparse
from urllib.request import Request, urlopen
from urllib.error import HTTPError


def main():
    root = Path('/usr/lib/simple-stt')
    assert os.geteuid() != 0, 'Run the installed smoke as an ordinary user'
    assert not os.access(root, os.W_OK), 'Package directory must be root-owned'
    names = ['simple-stt-linux', 'simple-stt-capture', 'simple-stt-infer',
             'simple-stt-ctl', 'simple-stt-settings', 'simple-stt-settings-window', 'linux-fast-paste',
             'readerd', 'readerctl', 'reader-tts', 'reader-ocr', 'reader-hud']
    for name in names:
        executable = root / name
        assert executable.is_file() and os.access(executable, os.X_OK), name
        assert not os.access(executable, os.W_OK), name
        result = subprocess.run(['ldd', str(executable)], capture_output=True, text=True, check=True)
        assert 'not found' not in result.stdout, result.stdout
    assert not (root / 'runtime').exists(), 'base package must contain no bundled inference runtime'
    assert (root / 'downloads.json').is_file()
    assert (root / 'lid.176.ftz').stat().st_size == 938013
    for name in ('fixtures/parakeet-smoke.wav', 'fixtures/asr/arabic-sa.wav'):
        with wave.open(str(root / name), 'rb') as audio:
            assert audio.getnframes() > 0 and audio.getnchannels() == 1, name
            assert audio.getframerate() == 16000, name
    symbols = subprocess.check_output(['nm', '-D', str(root / 'linux-fast-paste')], text=True)
    for feature in ['atspi_', 'g_dbus_']:
        assert feature in symbols, f'Paste helper missing {feature}'
    assert "XTestFakeKeyEvent" not in symbols, "Legacy XTest injection must not ship"
    for installed in ['/usr/bin/simple-stt', '/usr/lib/systemd/user/simple-stt-linux.service',
                      '/usr/share/applications/io.github.yosef0H4.simple_stt.desktop',
                      '/usr/share/applications/simple-stt-settings.desktop',
                      '/usr/share/applications/simple-stt-readerd.desktop']:
        assert Path(installed).is_file(), installed
    kwin_grant = Path('/usr/share/applications/simple-stt-readerd.desktop').read_text()
    assert 'Exec=/usr/lib/simple-stt/readerd' in kwin_grant
    assert 'X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2' in kwin_grant
    assert (root / 'linux-installation').is_file()
    with tempfile.TemporaryDirectory(prefix='simple-stt-package-') as temporary:
        home = Path(temporary)
        env = dict(os.environ, HOME=temporary, XDG_CONFIG_HOME=str(home / 'config'),
                   XDG_DATA_HOME=str(home / 'data'), XDG_CACHE_HOME=str(home / 'cache'),
                   XDG_RUNTIME_DIR=str(home / 'run'), XDG_SESSION_TYPE='wayland',
                   WAYLAND_DISPLAY='unavailable-package-test-display',
                   DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-package-test-bus')
        for name in ['SIMPLE_STT_RUNTIME_ROOT', 'SIMPLE_STT_CONFIG', 'SIMPLE_STT_BINDIR', 'DISPLAY']:
            env.pop(name, None)
        (home / 'run').mkdir(mode=0o700)
        subprocess.run(['/usr/bin/simple-stt', '--help'], cwd=home, env=env,
                       check=True, stdout=subprocess.DEVNULL, timeout=10)
        process = subprocess.Popen([str(root / 'simple-stt-settings'), '--no-browser'],
                                   cwd=home, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                assert selector.select(timeout=30), 'Settings did not announce its URL'
            parsed = urlparse(process.stdout.readline().strip())
            origin = f'{parsed.scheme}://{parsed.netloc}'
            token = parse_qs(parsed.fragment)['token'][0]
            request = Request(origin + '/api/state', headers={
                'Origin': origin, 'X-Simple-STT-Token': token})
            with urlopen(request, timeout=20) as response:
                state = json.load(response)
            assert state['config_path'] == str(home / 'config/simple-stt/instances/installed/config.json')
            assert state['config']['speech']['runtime_dir'] == ''
            assert state['resolved_model_dir'] == str(home / 'data/simple-stt/models')
            assert not state['runtime_ready']
            assert not state['service_online']
            assert state['linux_automation']['recommended'] == 'portal', 'installed input must use the portal'
            assert (root / 'linux-fast-paste').is_file(), 'read-only desktop helper missing'
            assert state['config']['speech']['single_model_filename'] is None
            assert 'model_setup' not in state
            assert state['config']['speech']['selection_mode']=='follow_keyboard'
            assert state['config']['speech']['language_models']=={}
            assert state['config']['speech']['language_runtimes']=={}
            (home / 'data/simple-stt/models').mkdir(parents=True, exist_ok=True)
            (home / 'data/simple-stt/models/write-test').write_text('user-writable')
            # A stopped portal shell must return an error, never report success
            # or launch a real desktop settings application in this isolated test.
            action = Request(origin + '/api/platform-action',
                             data=json.dumps({'action': 'configure_shortcuts'}).encode(),
                             headers={'Origin': origin, 'X-Simple-STT-Token': token,
                                      'Content-Type': 'application/json'})
            try:
                urlopen(action, timeout=15).close()
                raise AssertionError('Stopped shortcut shell incorrectly reported success')
            except HTTPError as error:
                assert error.code >= 400
                assert b'not running' in error.read(), 'Missing actionable shortcut error'
            # Exercise the real Settings -> shell request/result transport with
            # a fake portal responder. No session bus, keyboard or host UI is used.
            shell_data = home / 'data/simple-stt/instances/installed'
            (shell_data / 'linux-daemon.pid').write_text(str(os.getpid()))
            request_file = shell_data / 'linux-configure-shortcuts.request'
            result_file = shell_data / 'linux-configure-shortcuts.result'
            for reply, expected_status in [('portal shortcut configuration opened', 200),
                                            ('error: isolated portal failure', 400)]:
                failures = []
                def respond():
                    deadline = time.monotonic() + 10
                    while not request_file.exists() and time.monotonic() < deadline:
                        time.sleep(0.02)
                    if not request_file.exists():
                        failures.append('shell never requested portal configuration')
                        return
                    request_file.unlink()
                    result_file.write_text(reply)
                responder = threading.Thread(target=respond)
                responder.start()
                try:
                    with urlopen(action, timeout=15) as response:
                        assert response.status == expected_status
                except HTTPError as error:
                    assert error.code == expected_status
                    assert b'isolated portal failure' in error.read()
                finally:
                    responder.join(timeout=12)
                assert not failures and not responder.is_alive(), failures
                assert not result_file.exists(), 'portal result was not consumed'
            (shell_data / 'linux-daemon.pid').unlink()
            with urlopen(origin, timeout=10) as response:
                assert b'<html' in response.read().lower()
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
    print('PASS installed binaries, dependencies, ABI 6, full paste helper, embedded UI, stable profile and writable models')


if __name__ == '__main__':
    main()
