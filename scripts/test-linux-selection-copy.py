#!/usr/bin/env python3
"""Time real selection reads in a disposable Xvfb + private D-Bus session.

No host display, clipboard, session bus, microphone, or installed settings are used.
Requires Xvfb, dbus-run-session, xdotool, xclip, gdbus, GTK3 Python GI and
AT-SPI runtime components. Run after building resources/bin/linux-fast-paste.
"""
import argparse
import ast
import json
import os
from pathlib import Path
import shutil
import socket
import statistics
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'resources/bin/linux-fast-paste'
TARGET = ROOT / 'scripts/fixtures/selection-copy-target.py'
EXPECTED = 'مرحبا بالعالم Hello world'.encode()
ARTIFACT = ROOT / 'artifacts/combined-app/selection-copy-timings.json'
LAUNCHER = Path('/usr/libexec/at-spi-bus-launcher')
REGISTRY = Path('/usr/libexec/at-spi2-registryd')
READERD = ROOT / 'target/debug/readerd'


def wait_for(fn, description, seconds=6):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if fn():
            return
        time.sleep(.02)
    raise RuntimeError('timed out waiting for ' + description)


def stop(process):
    if process and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)


def run(command, env, **kwargs):
    return subprocess.run(command, env=env, timeout=4, capture_output=True, **kwargs)


def statistics_ms(values):
    values = sorted(values)
    return {'trials': len(values), 'median_ms': round(statistics.median(values), 2),
            'p95_ms': round(values[(95 * len(values) + 99) // 100 - 1], 2),
            'samples_ms': [round(x, 2) for x in values]}


def session(folder):
    assert os.environ.get('SIMPLE_STT_SELECTION_PRIVATE') == '1'
    root = Path(folder)
    env = dict(os.environ, NO_AT_BRIDGE='0', GTK_A11Y='always')
    launcher = registry = target = owner = daemon = None
    report = {'environment': 'private Xvfb and private D-Bus', 'cases': {},
              'skips': ['Kitty terminal: optional fixture not implemented',
                        'Chromium: optional fixture not implemented']}
    try:
        launcher = subprocess.Popen([str(LAUNCHER), '--launch-immediately'], env=env,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        address = None
        def bus_ready():
            nonlocal address
            response = run(['gdbus', 'call', '--session', '--dest', 'org.a11y.Bus',
                            '--object-path', '/org/a11y/bus', '--method', 'org.a11y.Bus.GetAddress'], env)
            if response.returncode == 0:
                address = ast.literal_eval(response.stdout.decode())[0]
            return bool(address)
        wait_for(bus_ready, 'private accessibility bus')
        env['AT_SPI_BUS_ADDRESS'] = address
        registry = subprocess.Popen([str(REGISTRY)], env=dict(env, DBUS_SESSION_BUS_ADDRESS=address),
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        target = subprocess.Popen(['/usr/bin/python3', str(TARGET), str(root)], env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sequence = 0
        def set_mode(mode):
            nonlocal sequence
            sequence += 1
            (root / 'command').write_text(f'{mode}:{sequence}')
            wait_for(lambda: (root / 'ready').exists() and (root / 'ready').read_text() == str(sequence),
                     'GTK target ' + mode)
            search = run(['xdotool', 'search', '--name', 'Simple STT private selection target'], env)
            if search.returncode:
                raise RuntimeError('GTK target window unavailable')
            focus = run(['xdotool', 'windowfocus', '--sync', search.stdout.decode().splitlines()[0]], env)
            if focus.returncode:
                raise RuntimeError('could not focus private GTK target')
            time.sleep(.08)
        set_mode('selected')
        sentinel = b'private-binary\0clipboard'
        owner = subprocess.Popen(['xclip', '-quiet', '-selection', 'clipboard', '-target',
                                  'application/x-simple-stt-test'], env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        owner.stdin.write(sentinel)
        owner.stdin.close()
        def clipboard(mime):
            return run(['xclip', '-selection', 'clipboard', '-out', '-target', mime], env)
        wait_for(lambda: clipboard('application/x-simple-stt-test').stdout == sentinel, 'binary clipboard')
        direct = []
        for _ in range(10):
            start = time.monotonic()
            result = run([str(HELPER), '--read-selection'], env)
            direct.append((time.monotonic() - start) * 1000)
            if result.returncode != 0 or result.stdout != EXPECTED:
                raise AssertionError(f'AT-SPI selection failed: {result.returncode} {result.stdout!r} {result.stderr!r}')
        assert clipboard('application/x-simple-stt-test').stdout == sentinel, 'direct selection altered binary clipboard'
        report['cases']['atspi'] = statistics_ms(direct)
        for mode, expected_exit in [('empty', 2), ('password', 1), ('button', 1)]:
            set_mode(mode)
            response = run([str(HELPER), '--read-selection'], env)
            assert response.returncode == expected_exit and not response.stdout, (mode, response.returncode, response.stdout)
            report['cases'][mode] = 'pass'
        for chord in ('ctrl-insert', 'ctrl-c'):
            timings = []
            for _ in range(10):
                set_mode('selected')
                # Install a different value so a failed copy cannot pass with stale data.
                stop(owner)
                owner = subprocess.Popen(['xclip', '-quiet', '-selection', 'clipboard'], env=env,
                                         stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL)
                owner.stdin.write(b'sentinel')
                owner.stdin.close()
                wait_for(lambda: clipboard('UTF8_STRING').stdout == b'sentinel', 'clipboard sentinel')
                start = time.monotonic()
                sent = run([str(HELPER), '--copy-key', chord], env)
                assert sent.returncode == 0, (chord, sent.stderr)
                def copied():
                    value = clipboard('UTF8_STRING')
                    return value.returncode == 0 and value.stdout == EXPECTED
                wait_for(copied, chord + ' selected text', seconds=3)
                timings.append((time.monotonic() - start) * 1000)
            report['cases'][chord] = statistics_ms(timings)

        # Exercise the controller through its local protocol. Empty model
        # assignments deliberately stop execution before audio or inference.
        data = root / 'data'
        reader = data / 'reader'
        reader.mkdir(parents=True)
        config_file = reader / 'config.json'
        daemon_env = dict(env, XDG_DATA_HOME=str(data), XDG_CONFIG_HOME=str(root / 'config'),
                          XDG_CACHE_HOME=str(root / 'cache'), SIMPLE_STT_READER_DATA=str(reader),
                          SIMPLE_TTS_CONFIG=str(config_file), SIMPLE_TTS_NO_DESKTOP='1')
        def config(backend, allow=True, overrides=None):
            config_file.write_text(json.dumps({
                'selection_copy': {'backend': backend, 'shortcut': 'ctrl_insert',
                                   'allow_clipboard_copy': allow, 'show_clipboard_notice': False},
                'selection_copy_overrides': overrides or [],
                'privacy': {'hide_text_in_notifications': False},
            }))

        config('auto')
        daemon = subprocess.Popen([str(READERD), 'daemon', '--foreground', '--no-desktop'],
                                  env=daemon_env, stdin=subprocess.DEVNULL,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sockpath = reader / 'simple-tts.sock'
        wait_for(lambda: sockpath.exists() or daemon.poll() is not None, 'private reader controller')
        assert daemon.poll() is None, 'private reader controller exited'

        def request(value):
            payload = json.dumps(value).encode()
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(4)
                client.connect(str(sockpath))
                client.sendall(struct.pack('<I', len(payload) + 1) + b'\0' + payload)
                def read_exact(length):
                    chunks = bytearray()
                    while len(chunks) < length:
                        chunk = client.recv(length - len(chunks))
                        if not chunk:
                            raise RuntimeError('controller closed its response')
                        chunks.extend(chunk)
                    return bytes(chunks)
                length = struct.unpack('<I', read_exact(4))[0]
                assert 2 <= length <= 65536
                body = read_exact(length)
                assert body[:1] == b'\0'
                response = json.loads(body[1:])
                assert response['ok'], response
                return response

        def action():
            old = request({'op': 'status'})['state']['notice_revision']
            request({'op': 'action', 'action': 'read-selection'})
            def finished():
                state = request({'op': 'status'})['state']
                if not state['read_pending'] and state['notice_revision'] != old and state['notice_text']:
                    return state
                return None
            end = time.monotonic() + 10
            while time.monotonic() < end:
                state = finished()
                if state:
                    return state['notice_text']
                time.sleep(.03)
            raise AssertionError('controller selection action did not finish')

        def binary_clipboard():
            nonlocal owner
            stop(owner)
            owner = subprocess.Popen(['xclip', '-quiet', '-selection', 'clipboard', '-target',
                                      'application/x-simple-stt-test'], env=env, stdin=subprocess.PIPE,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            owner.stdin.write(sentinel)
            owner.stdin.close()
            wait_for(lambda: clipboard('application/x-simple-stt-test').stdout == sentinel,
                     'private binary clipboard')

        set_mode('selected')
        binary_clipboard()
        notice = action()
        assert 'missing' in notice.lower(), notice  # Selection reached TTS routing.
        assert clipboard('application/x-simple-stt-test').stdout == sentinel, 'auto modified clipboard'
        report['cases']['controller_auto_binary_preserved'] = 'pass'

        config('native')
        for iteration in range(2):
            set_mode('selected')
            notice = action()
            assert 'missing' in notice.lower(), (iteration, notice)
            assert clipboard('UTF8_STRING').stdout == EXPECTED, 'native copied wrong selection'
        report['cases']['controller_native_same_value_twice'] = 'pass'

        for mode in ('empty', 'button'):
            set_mode(mode)
            notice = action()
            assert 'did not copy' in notice.lower(), (mode, notice)
            assert clipboard('UTF8_STRING').stdout == EXPECTED, 'old clipboard was unexpectedly changed'
            report['cases']['controller_native_' + mode] = 'pass'

        set_mode('selected')
        config('native', allow=False)
        notice = action()
        assert 'enable shortcut copy' in notice.lower(), notice
        assert clipboard('UTF8_STRING').stdout == EXPECTED, 'disabled copy changed clipboard'
        report['cases']['controller_copy_disabled'] = 'pass'

        app_id = run([str(HELPER), '--active-app'], env)
        identity = app_id.stdout.decode().strip() if app_id.returncode == 0 else ''
        if identity:
            # Accessibility-only globally, but use the explicitly identified
            # target's native shortcut rule for a copy that changes ownership.
            config('accessibility', overrides=[{'app_id': identity, 'backend': 'native',
                                                'shortcut': 'ctrl_insert'}])
            binary_clipboard()
            set_mode('selected')
            notice = action()
            assert 'missing' in notice.lower(), notice
            assert clipboard('UTF8_STRING').stdout == EXPECTED
            report['cases']['controller_app_override'] = 'pass'
        else:
            report['skips'].append('controller app override: active app identity unavailable')
        return report
    finally:
        for process in (daemon, target, owner, registry, launcher):
            stop(process)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--session', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.session:
        print(json.dumps(session(args.session), ensure_ascii=False))
        return
    missing = [name for name in ('Xvfb', 'dbus-run-session', 'xdotool', 'xclip', 'gdbus') if not shutil.which(name)]
    missing += [str(path) for path in (HELPER, READERD, LAUNCHER, REGISTRY, Path('/usr/bin/python3')) if not path.exists()]
    if missing:
        print('SKIP missing dependencies: ' + ', '.join(missing))
        return
    probe = subprocess.run(['/usr/bin/python3', '-c', "import gi; gi.require_version('Gtk','3.0'); from gi.repository import Gtk"], capture_output=True, timeout=5)
    if probe.returncode:
        print('SKIP GTK3 Python GI unavailable')
        return
    with tempfile.TemporaryDirectory(prefix='simple-stt-selection-x11-') as folder:
        root = Path(folder)
        runtime = root / 'runtime'
        runtime.mkdir(mode=0o700)
        display_file = root / 'display'
        with display_file.open('w+') as display:
            server = subprocess.Popen(['Xvfb', '-displayfd', str(display.fileno()), '-screen', '0',
                                       '1024x768x24', '-nolisten', 'tcp'], pass_fds=(display.fileno(),),
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                wait_for(lambda: bool(display_file.read_text().strip()) or server.poll() is not None, 'Xvfb')
                if server.poll() is not None:
                    raise RuntimeError('private Xvfb exited')
                env = os.environ.copy()
                for name in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'AT_SPI_BUS_ADDRESS', 'DBUS_SESSION_BUS_ADDRESS'):
                    env.pop(name, None)
                env.update(DISPLAY=':' + display_file.read_text().strip(), XDG_RUNTIME_DIR=str(runtime),
                           XDG_SESSION_TYPE='x11', GDK_BACKEND='x11', SIMPLE_STT_SELECTION_PRIVATE='1')
                result = subprocess.run(['dbus-run-session', '--', sys.executable, str(Path(__file__).resolve()),
                                         '--session', folder], env=env, capture_output=True, text=True, timeout=100)
                if result.returncode:
                    raise RuntimeError(f'private session failed: {result.stderr}\n{result.stdout}')
                report = json.loads(result.stdout)
                ARTIFACT.parent.mkdir(parents=True, exist_ok=True)
                ARTIFACT.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
                print(json.dumps(report, indent=2, ensure_ascii=False))
            finally:
                stop(server)


if __name__ == '__main__':
    main()
