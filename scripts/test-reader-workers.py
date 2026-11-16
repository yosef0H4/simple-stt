#!/usr/bin/env python3
"""Worker-only acceptance: no desktop, audio output, clipboard, or microphone."""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import struct
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]


def send(child, value):
    body = b'\0' + json.dumps(value).encode()
    child.stdin.write(struct.pack('<I', len(body)) + body)
    child.stdin.flush()


def read_exact(stream, count):
    result = b''
    while len(result) < count:
        part = stream.read(count - len(result))
        if not part:
            raise EOFError('Worker closed its output')
        result += part
    return result


def events(child, output):
    try:
        while True:
            size = struct.unpack('<I', read_exact(child.stdout, 4))[0]
            assert 1 < size <= 65536
            frame = read_exact(child.stdout, size)
            if frame[0] == 0:
                output.put(('control', json.loads(frame[1:]), b''))
            else:
                assert frame[0] == 1
                length = struct.unpack('<I', frame[1:5])[0]
                header = json.loads(frame[5:5 + length])
                pcm = frame[5 + length:]
                assert len(pcm) == header['sample_count'] * 2
                output.put(('pcm', header, pcm))
    except Exception as error:
        output.put(('error', str(error), b''))


def launch(binary, arguments, env, log):
    child = subprocess.Popen([str(binary), *arguments], env=env, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=log)
    output = queue.Queue()
    threading.Thread(target=events, args=(child, output), daemon=True).start()
    send(child, {'type': 'hello', 'protocol': 1, 'incarnation': 'isolated-worker'})
    kind, ready, _ = output.get(timeout=10)
    assert kind == 'control' and ready['type'] == 'ready', ready
    return child, output


def finish(child):
    try:
        if child.poll() is not None:
            raise AssertionError(f'Worker exited {child.returncode}; see artifacts/combined-app/worker.log')
        send(child, {'type': 'shutdown'})
        child.wait(timeout=5)
        assert child.returncode == 0
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--ocr-model', type=Path, required=True)
    parser.add_argument('--ocr-runtime', type=Path, required=True)
    parser.add_argument('--device', choices=['cpu', 'gpu'], default='gpu')
    parser.add_argument('--model-dir', type=Path)
    parser.add_argument('--runtime-dir', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='simple-stt-reader-worker-') as temporary:
        scratch = Path(temporary) / 'capture'
        scratch.mkdir(mode=0o700)
        for theme in ['light', 'dark', 'arabic']:
            shutil.copyfile(ROOT / f'scripts/fixtures/reader/ocr-{theme}.png', scratch / f'{theme}.png')
        env = dict(os.environ, XDG_DATA_HOME=temporary, XDG_CACHE_HOME=temporary, SIMPLE_TTS_CAPTURE_DIR=str(scratch),
                   WAYLAND_DISPLAY='unavailable-test-display', DISPLAY='',
                   DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus')
        runtime_lib = args.ocr_runtime / 'lib' if (args.ocr_runtime / 'lib').is_dir() else args.ocr_runtime
        env['LD_LIBRARY_PATH'] = str(runtime_lib) + os.pathsep + env.get('LD_LIBRARY_PATH', '')
        log_dir = ROOT / 'artifacts/combined-app'
        log_dir.mkdir(parents=True, exist_ok=True)
        with open(log_dir / 'worker.log', 'wb') as log:
            child, output = launch(args.bin_dir / 'reader-ocr', ['--model-dir', str(args.ocr_model), '--runtime-dir', str(args.ocr_runtime), '--device', args.device], env, log)
            try:
                for theme in ['light', 'dark', 'arabic']:
                    send(child, {'type': 'recognize', 'job_id': theme, 'generation': 8,
                                 'image_path': str(scratch / f'{theme}.png'),
                                 'language': 'eng'})
                    kind, recognized, _ = output.get(timeout=20)
                    assert kind == 'control' and recognized['type'] == 'text', recognized
                    if theme == 'arabic':
                        compact = ''.join(c for c in recognized['text'] if c.isalnum())
                        arabic = ''.join(c for c in compact if ord(c) >= 0x0600)
                        import difflib
                        assert arabic.startswith('مرحبابالعالم'), recognized
                        assert difflib.SequenceMatcher(None, 'مرحبابالعالمهذااختبارباللغةالعربية', arabic).ratio() >= .95, recognized
                        assert 'SimpleSTTreadsArabicandEnglish' in compact, recognized
                    else:
                        assert 'Simple TTS reads this screenshot.' in recognized['text'], recognized
                    assert output.get(timeout=5)[1]['type'] == 'finished'
            finally:
                finish(child)
            print('PASS real OCR worker: light/dark fixtures, exact text, clean shutdown')
            if args.model_dir and args.runtime_dir:
                env['LD_LIBRARY_PATH'] = str(args.runtime_dir / 'lib') + os.pathsep + env.get('LD_LIBRARY_PATH', '')
                child, output = launch(args.bin_dir / 'reader-tts', ['--model-dir', str(args.model_dir),
                                       '--runtime-dir', str(args.runtime_dir), '--device', 'cuda' if args.device == 'gpu' else 'cpu'], env, log)
                try:
                    send(child, {'type': 'synthesize', 'job_id': 'test', 'generation': 7,
                                 'sentence': 0, 'unit': 0, 'text': 'Hello world.',
                                 'voice': 'af_heart', 'language': 'en-us', 'speed': 1.0, 'device': 'cuda' if args.device == 'gpu' else 'cpu'})
                    sample_count = 0
                    while True:
                        kind, header, pcm = output.get(timeout=50)
                        assert kind != 'error', header
                        if kind == 'pcm':
                            assert header['sample_offset'] == sample_count
                            assert header['sample_rate'] == 24000
                            assert header['generation'] == 7
                            sample_count += len(pcm) // 2
                        else:
                            assert header['type'] != 'error', header
                            if header['type'] == 'finished':
                                break
                    assert sample_count > 12000
                finally:
                    finish(child)
                print(f'PASS real TTS worker: {sample_count} PCM samples captured without playback')
            else:
                print('SKIP real voice inference: supply --model-dir and --runtime-dir')


if __name__ == '__main__':
    main()
