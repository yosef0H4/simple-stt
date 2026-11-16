#!/usr/bin/env python3
"""Private GPU copy-region lifecycle regression; no host picker, clipboard or audio.

Requires built binaries, installed Paddle assets and Bubblewrap. No models are
 downloaded. Run explicitly; this script does not change the installed app.
"""
import argparse
import glob
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('reader_service_test', ROOT / 'scripts/test-reader-service.py')
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)


def make_executable(path, text):
    path.write_text(text)
    path.chmod(0o700)


def stage_runtime(assets, source, catalog):
    entry = catalog['sherpa-cuda']
    service.stage_asset(assets, 'sherpa-cuda', source, ['lib'], entry['sha256'])
    for dep in entry.get('dependencies', []):
        asset = catalog[dep]
        destination = assets / dep
        destination.mkdir(parents=True, exist_ok=True)
        for name in asset['required']:
            target = source / 'lib' / Path(name).name
            if not target.exists():
                raise FileNotFoundError(target)
            link = destination / name
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(target.resolve())
        (destination / '.verified').write_text(asset['sha256'])


def isolated(args, private):
    private = Path(private)
    data = private / 'data'
    reader = data / 'simple-stt' / 'reader'
    reader.mkdir(parents=True, exist_ok=True)
    for name in ['config', 'cache', 'runtime', 'home', 'xdg-runtime']:
        (private / name).mkdir(exist_ok=True, mode=0o700)
    (private / 'runtime/linux-installation').touch()
    catalog = {entry['id']: entry for entry in json.loads((ROOT / 'assets/catalog/downloads.json').read_text())}
    assets = reader.parent / 'assets'
    model = catalog['paddle-ar-en']
    service.stage_asset(assets, 'paddle-ar-en', args.ocr_model.resolve(), model['required'], model['sha256'])
    stage_runtime(assets, args.ocr_runtime.resolve(), catalog)
    instance = data / 'simple-stt/instances/installed'
    instance.mkdir(parents=True)
    (instance / 'assets').symlink_to(assets)
    config = {
        'ocr': {
            'selection': {'mode': 'single_model', 'single': {'runtime': 'paddle-cuda', 'model': 'paddle-ar-en', 'device': 'gpu'}},
            'idle_timeout_secs': 1,
        },
        'playback': {'keep_warm_seconds': 30},
    }
    (reader / 'config.json').write_text(json.dumps(config))
    env = os.environ.copy()
    env.update(HOME=str(private / 'home'), XDG_DATA_HOME=str(data),
               XDG_CONFIG_HOME=str(private / 'config'), XDG_CACHE_HOME=str(private / 'cache'),
               XDG_RUNTIME_DIR=str(private / 'xdg-runtime'), SIMPLE_STT_RUNTIME_ROOT=str(private / 'runtime'),
               SIMPLE_STT_READER_DATA=str(reader), SIMPLE_TTS_CONFIG=str(reader / 'config.json'),
               SIMPLE_TTS_NO_DESKTOP='1', SIMPLE_STT_NO_DESKTOP='1', DISPLAY='',
               WAYLAND_DISPLAY='private-unavailable-wayland',
               DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent-private-test-bus',
               PATH=str(private / 'bin') + ':/usr/bin:/bin',
               COPY_TEST_DIR=str(private), COPY_TEST_FIXTURE=str(service.FIXTURE))
    binary = args.bin_dir.resolve()
    binaries = {name: binary / name for name in ('simple-stt-linux', 'readerctl', 'readerd', 'reader-ocr')}
    missing = [str(path) for path in binaries.values() if not path.is_file()]
    if missing:
        raise SystemExit('missing built binaries: ' + ', '.join(missing))
    picker_ready = private / 'picker-ready'
    picker_release = private / 'picker-release'
    clipboard = private / 'clipboard'
    observed = set()
    with (private / 'readerd.log').open('w+') as log:
        daemon = subprocess.Popen([str(binaries['readerd']), 'daemon', '--foreground', '--no-desktop'],
                                  env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        monitor = service.WorkerMonitor(daemon.pid)
        try:
            def socket_ready():
                if daemon.poll() is not None:
                    log.flush()
                    raise AssertionError('private reader exited: ' + (private / 'readerd.log').read_text())
                return (reader / 'simple-tts.sock').exists()
            service.wait_for(socket_ready, 10, 'private reader socket')

            def status():
                return json.loads(service.run([binaries['readerctl'], 'status', '--json'], env))

            def start_picker():
                picker_ready.unlink(missing_ok=True)
                picker_release.unlink(missing_ok=True)
                service.run([binaries['simple-stt-linux'], 'copy-region'], env, timeout=30)
                service.wait_for(picker_ready.exists, 10, 'fake picker to open')

            def ready_ocr():
                pid = service.worker(daemon.pid, 'reader-ocr')
                events = status().get('model_events', [])
                if pid and any(event.get('feature') == 'ocr' and event.get('event') == 'loaded' for event in events):
                    return pid
                return None

            start_picker()
            first = service.wait_for(ready_ocr, 120, 'OCR model loaded while picker is held')
            observed.add(first)
            tick = service.start_tick(first)
            assert tick is not None and not picker_release.exists()
            time.sleep(1.5)  # Longer than the configured OCR idle timeout.
            assert service.start_tick(first) == tick and service.alive(first), 'prewarmed OCR exited while picker open'
            assert not monitor.since(0, 'reader-tts'), 'copy-only prewarm started a voice worker'
            assert not clipboard.exists(), 'clipboard changed before selection'
            picker_release.touch()
            service.wait_for(clipboard.exists, 120, 'first private clipboard result')
            assert clipboard.read_text().strip() == service.EXPECTED
            assert service.start_tick(first) == tick and service.alive(first)

            # Start again promptly, before the one-second idle lease expires.
            clipboard.unlink()
            start_picker()
            assert service.worker(daemon.pid, 'reader-ocr') == first and service.start_tick(first) == tick, 'second copy did not reuse OCR PID'
            time.sleep(1.5)
            assert service.start_tick(first) == tick and service.alive(first), 'reused OCR exited during selection'
            picker_release.touch()
            service.wait_for(clipboard.exists, 120, 'second private clipboard result')
            assert clipboard.read_text().strip() == service.EXPECTED
            assert service.start_tick(first) == tick and service.alive(first)

            # The shared Abort dispatches the reader's stop action. Calling that
            # action directly avoids starting the unrelated audio capture service.
            clipboard.unlink()
            start_picker()
            service.run([binaries['readerctl'], 'stop'], env, timeout=15)
            service.wait_for(lambda: not service.alive(first) or service.start_tick(first) != tick,
                             10, 'OCR worker idle expiry after abort')
            picker_release.touch()
            time.sleep(0.5)
            assert not clipboard.exists(), 'aborted picker published clipboard text'
            assert not monitor.since(0, 'reader-tts'), 'copy action started a TTS worker'
            print(f'PASS isolated copy picker, GPU OCR warm reuse ({first}/{tick}), abort and idle expiry', flush=True)
        finally:
            monitor.close()
            observed.update(monitor.since(0, 'reader-ocr'))
            if daemon.poll() is None:
                try:
                    service.run([binaries['readerctl'], 'shutdown'], env, timeout=8)
                    daemon.wait(timeout=8)
                except (AssertionError, subprocess.TimeoutExpired):
                    daemon.kill()
                    daemon.wait(timeout=3)
            for pid in observed:
                if monitor.owned_alive(pid):
                    os.kill(pid, 9)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--ocr-model', type=Path, default=ROOT / 'artifacts/combined-app/downloads/model-paddle')
    parser.add_argument('--ocr-runtime', type=Path, default=Path.home() / '.local/share/simple-tts/runtime')
    parser.add_argument('--inside', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        isolated(args, args.inside)
        return
    bwrap = shutil.which('bwrap')
    if not bwrap:
        print('SKIP isolated copy-region regression: Bubblewrap is unavailable', flush=True)
        return
    if not Path('/usr/bin/lumine-capture').is_file():
        print('SKIP isolated copy-region regression: Bubblewrap needs the existing picker mount target', flush=True)
        return
    catalog = {entry['id']: entry for entry in json.loads((ROOT / 'assets/catalog/downloads.json').read_text())}
    missing = [args.ocr_model / name for name in catalog['paddle-ar-en']['required'] if not (args.ocr_model / name).exists()]
    if not (args.ocr_runtime / 'lib/libonnxruntime.so').exists():
        missing.append(args.ocr_runtime / 'lib/libonnxruntime.so')
    if missing:
        print('SKIP isolated copy-region regression: missing installed assets: ' + ', '.join(map(str, missing)), flush=True)
        return
    devices = [path for path in glob.glob('/dev/nvidia*') if Path(path).is_char_device()]
    if not devices:
        print('SKIP isolated copy-region regression: NVIDIA devices unavailable', flush=True)
        return
    with tempfile.TemporaryDirectory(prefix='simple-stt-copy-region-') as tmp:
        private = Path(tmp)
        (private / 'bin').mkdir()
        picker = private / 'fake-picker'
        make_executable(picker, '#!/bin/sh\nset -eu\n: > "$COPY_TEST_DIR/picker-ready"\nwhile [ ! -e "$COPY_TEST_DIR/picker-release" ]; do /bin/sleep 0.05; done\nexec /bin/cat "$COPY_TEST_FIXTURE"\n')
        make_executable(private / 'bin/wl-copy', '#!/bin/sh\nset -eu\n/bin/cat > "$COPY_TEST_DIR/clipboard.tmp"\n/bin/mv "$COPY_TEST_DIR/clipboard.tmp" "$COPY_TEST_DIR/clipboard"\n')
        command = [bwrap, '--die-with-parent', '--unshare-net', '--ro-bind', '/', '/',
                   '--bind', tmp, tmp, '--proc', '/proc', '--dev', '/dev',
                   '--ro-bind', str(picker), '/usr/bin/lumine-capture']
        for device in devices:
            command += ['--dev-bind', device, device]
        command += [sys.executable, str(Path(__file__).resolve()), '--inside', tmp,
                    '--bin-dir', str(args.bin_dir.resolve()), '--ocr-model', str(args.ocr_model.resolve()),
                    '--ocr-runtime', str(args.ocr_runtime.resolve())]
        subprocess.run(command, check=True, timeout=300)


if __name__ == '__main__':
    main()
