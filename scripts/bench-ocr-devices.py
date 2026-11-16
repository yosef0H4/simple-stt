#!/usr/bin/env python3
"""Compare warm CPU and strict-GPU OCR on private, public-text image fixtures.

Requires Pillow and already-built reader-ocr plus the model and runtimes. This
never opens a display, reads the clipboard, captures audio, or uses user files.
"""
import argparse
import csv
import difflib
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time
import unicodedata
from collections import defaultdict

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'artifacts/combined-app/benchmarks'
FIXTURES = ROOT / 'scripts/fixtures/reader'
SIZES = [(640, 240), (1280, 720), (1920, 1080), (3840, 2160)]


def load_protocol():
    spec = importlib.util.spec_from_file_location('worker_helpers', ROOT / 'scripts/test-reader-workers.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def create_cases(directory):
    try:
        from PIL import Image, ImageDraw, ImageFont, features
    except ImportError as error:
        raise RuntimeError('Pillow is required to compose the committed OCR fixtures') from error
    cases = []
    for name in ('arabic', 'dark'):
        original = directory / f'verified-{name}.png'
        shutil.copyfile(FIXTURES / f'ocr-{name}.png', original)
        with Image.open(original) as image:
            source = image.convert('RGB')
            cases.append({'id': f'verified-{name}', 'path': original, 'language': name,
                          'density': 'original', 'width': source.width, 'height': source.height,
                          'tiles': 1, 'source': original.name})
            # Reuse the verified screenshot instead of rendering new fonts. Tiles
            # are not stretched; sparse and dense cases differ only in count.
            for width, height in SIZES:
                for density in ('sparse', 'dense'):
                    tile = source.copy()
                    max_height = (height - 40) // 2 if density == 'dense' else height - 24
                    tile.thumbnail((min(width - 24, 800), min(max_height, 360)), Image.Resampling.LANCZOS)
                    bg = tile.getpixel((0, 0))
                    canvas = Image.new('RGB', (width, height), bg)
                    step_x, step_y = tile.width + 16, tile.height + 16
                    positions = [(12, 12)] if density == 'sparse' else [
                        (x, y) for y in range(12, height - tile.height + 1, step_y)
                        for x in range(12, width - tile.width + 1, step_x)]
                    for x, y in positions:
                        canvas.paste(tile, (x, y))
                    case_id = f'{name}-{width}x{height}-{density}'
                    path = directory / f'{case_id}.png'
                    canvas.save(path, optimize=True)
                    cases.append({'id': case_id, 'path': path, 'language': name, 'density': density,
                                  'width': width, 'height': height, 'tiles': len(positions),
                                  'tile_width': tile.width, 'tile_height': tile.height,
                                  'source': original.name})
    # Independent small selections, rather than more crops of the same source.
    # RAQM is required so the Arabic fixtures contain correctly joined glyphs.
    if not features.check('raqm'):
        raise RuntimeError('Pillow RAQM support is required for shaped Arabic fixtures')
    samples = [
        ('english-chat', 'en', 22, False, ['Please send the report tomorrow.', 'The meeting starts at ten.']),
        ('english-notice', 'en', 18, True, ['Your download is complete.', 'Open the file to continue.']),
        ('english-code', 'en', 20, True, ['Shared: code, heap, open files.', 'Private: registers and stack.']),
        ('english-large', 'en', 32, False, ['Read selected text aloud.']),
        ('arabic-chat', 'ar', 24, False, ['يرجى إرسال التقرير غدا.', 'يبدأ الاجتماع في الساعة العاشرة.']),
        ('arabic-notice', 'ar', 20, True, ['اكتمل التنزيل بنجاح.', 'افتح الملف للمتابعة.']),
        ('arabic-large', 'ar', 32, False, ['اقرأ النص المحدد بصوت واضح.']),
        ('mixed-settings', 'ar', 22, True, ['اختر النموذج المناسب للقراءة.', 'Kokoro English / Supertonic Arabic']),
    ]
    for name, language, size, dark, lines in samples:
        width, height = 760, 40 + len(lines) * (size * 2)
        canvas = Image.new('RGB', (width, height), '#171c24' if dark else '#ffffff')
        draw = ImageDraw.Draw(canvas)
        for index, line in enumerate(lines):
            rtl = language == 'ar' and any('\u0600' <= char <= '\u06ff' for char in line)
            font = ImageFont.truetype(str(ROOT / 'assets/fonts' / ('NotoSansArabic-Regular.ttf' if rtl else 'JetBrainsMono-Regular.ttf')), size)
            draw.text((width - 20 if rtl else 20, 15 + index * size * 2), line, font=font,
                      fill='#e8edf4' if dark else '#171c24', anchor='ra' if rtl else 'la',
                      direction='rtl' if rtl else 'ltr')
        path = directory / f'small-{name}.png'
        canvas.save(path)
        cases.append({'id': f'small-{name}', 'path': path, 'language': language, 'density': 'small-selection',
                      'width': width, 'height': height, 'tiles': 1, 'source': 'independent public sample',
                      'truth': '\n'.join(lines), 'expected_fragments': [normalize(line) for line in lines]})
    return cases


def memory(pid):
    result = {'pss_kib': None, 'rss_kib': None, 'gpu_mib': None}
    for filename, fields in [('smaps_rollup', {'Pss:': 'pss_kib', 'Rss:': 'rss_kib'}),
                             ('status', {'VmRSS:': 'rss_kib'})]:
        try:
            for line in Path(f'/proc/{pid}/{filename}').read_text().splitlines():
                parts = line.split()
                if parts and parts[0] in fields and result[fields[parts[0]]] is None:
                    result[fields[parts[0]]] = int(parts[1])
        except (OSError, ValueError, IndexError):
            pass
    try:
        output = subprocess.run(['nvidia-smi', '--query-compute-apps=pid,used_gpu_memory',
                                 '--format=csv,noheader,nounits'], capture_output=True,
                                text=True, timeout=2, check=True).stdout
        for line in output.splitlines():
            columns = line.split(',')
            if len(columns) >= 2 and columns[0].strip() == str(pid):
                result['gpu_mib'] = int(columns[1].strip())
                break
    except (OSError, subprocess.SubprocessError, ValueError):
        pass
    return result


def normalize(text):
    return ''.join(c for c in unicodedata.normalize('NFKC', text).casefold() if c.isalnum())


def ort_version(runtime):
    libdir = runtime / 'lib' if (runtime / 'lib').is_dir() else runtime
    library = (libdir / 'libonnxruntime.so').resolve(strict=True)
    code = '''import ctypes, sys
class ApiBase(ctypes.Structure):
    _fields_ = [("get_api", ctypes.c_void_p), ("get_version", ctypes.CFUNCTYPE(ctypes.c_char_p))]
lib = ctypes.CDLL(sys.argv[1])
lib.OrtGetApiBase.restype = ctypes.POINTER(ApiBase)
print(lib.OrtGetApiBase().contents.get_version().decode())
'''
    env = dict(os.environ, LD_LIBRARY_PATH=str(libdir) + os.pathsep + os.environ.get('LD_LIBRARY_PATH', ''))
    output = subprocess.run([os.sys.executable, '-c', code, str(library)], env=env, capture_output=True,
                            text=True, check=True, timeout=10)
    return {'version': output.stdout.strip(), 'library': str(library),
            'sha256': hashlib.sha256(library.read_bytes()).hexdigest()}


def receive(output, timeout):
    try:
        kind, value, _ = output.get(timeout=timeout)
    except queue.Empty as error:
        raise TimeoutError(f'worker did not respond within {timeout}s') from error
    if kind != 'control' or value.get('type') == 'error':
        raise RuntimeError(f'worker error: {value}')
    return value


def recognize(protocol, child, output, case, job):
    protocol.send(child, {'type': 'recognize', 'job_id': job, 'generation': 1,
                          'image_path': str(case['path']), 'language': 'eng'})
    deadline = time.monotonic() + 90
    text = None
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(f'{case["id"]} exceeded 90 seconds')
        event = receive(output, remaining)
        if event.get('job_id') != job:
            raise RuntimeError(f'unexpected OCR event: {event}')
        if event.get('type') == 'text':
            text = event['text']
        elif event.get('type') == 'finished':
            if text is None:
                raise RuntimeError('worker finished without recognized text')
            return text


def run_block(protocol, device, number, runtime, model, binary, cases, scratch, results):
    env = dict(os.environ, XDG_DATA_HOME=str(scratch), XDG_CACHE_HOME=str(scratch),
               XDG_CONFIG_HOME=str(scratch), XDG_RUNTIME_DIR=str(scratch),
               CUDA_CACHE_PATH=str(scratch / 'cuda-cache'),
               SIMPLE_TTS_CAPTURE_DIR=str(scratch), SIMPLE_TTS_NO_DESKTOP='1',
               DISPLAY='', WAYLAND_DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus')
    lib = runtime / 'lib' if (runtime / 'lib').is_dir() else runtime
    env['LD_LIBRARY_PATH'] = str(lib) + os.pathsep + env.get('LD_LIBRARY_PATH', '')
    child = None
    started = time.perf_counter()
    with (OUTPUT / f'{device}-block-{number}.log').open('wb') as log:
        try:
            child = subprocess.Popen([str(binary), '--model-dir', str(model), '--runtime-dir',
                                      str(runtime), '--device', device], env=env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log)
            output = queue.Queue()
            threading.Thread(target=protocol.events, args=(child, output), daemon=True).start()
            protocol.send(child, {'type': 'hello', 'protocol': 1, 'incarnation': f'{device}-{number}'})
            ready = receive(output, 10)
            if ready.get('type') != 'ready':
                raise RuntimeError(f'unexpected handshake: {ready}')
            recognize(protocol, child, output, cases[0], f'cold-{number}')
            cold_ms = (time.perf_counter() - started) * 1000
            if device == 'gpu':
                log.flush()
                if 'PaddleOCR CUDA verified' not in (OUTPUT / f'{device}-block-{number}.log').read_text(errors='replace'):
                    raise RuntimeError('strict GPU execution was not verified in the worker log')
            results['blocks'].append({'device': device, 'block': number, 'pid': child.pid,
                                      'cold_first_recognition_ms': cold_ms, 'memory_after_cold': memory(child.pid)})
            for i in range(3):
                recognize(protocol, child, output, cases[0], f'warm-{number}-{i}')
            for case in cases:
                print(f'block={number} device={device} case={case["id"]} shape warmup', flush=True)
                recognize(protocol, child, output, case, f'shape-{number}-{case["id"]}')
                for repetition in range(3):
                    start = time.perf_counter()
                    text = recognize(protocol, child, output, case, f'{number}-{case["id"]}-{repetition}')
                    elapsed_ms = (time.perf_counter() - start) * 1000
                    normalized = normalize(text)
                    results['runs'].append({'device': device, 'block': number, 'case': case['id'],
                                            'repetition': repetition + 1, 'pid': child.pid,
                                            'elapsed_ms': elapsed_ms, 'text': text,
                                            'normalized_sha256': hashlib.sha256(normalized.encode()).hexdigest(),
                                            'normalized_length': len(normalized), **memory(child.pid)})
                recent = results['runs'][-3:]
                print(f'block={number} device={device} case={case["id"]} ms={[round(r["elapsed_ms"],1) for r in recent]}', flush=True)
        finally:
            if child is not None:
                if child.poll() is None:
                    try:
                        protocol.send(child, {'type': 'shutdown'})
                        child.wait(timeout=5)
                    except (BrokenPipeError, OSError, subprocess.TimeoutExpired):
                        child.kill()
                        child.wait()
                if child.stdin:
                    child.stdin.close()
                if child.stdout:
                    child.stdout.close()


def summarize(results):
    cases = {case['id']: case for case in results['cases']}
    groups = defaultdict(list)
    for run in results['runs']:
        groups[(run['case'], run['device'])].append(run)
    summaries = []
    for (case, device), runs in sorted(groups.items()):
        times = sorted(run['elapsed_ms'] for run in runs)
        other = groups.get((case, 'gpu' if device == 'cpu' else 'cpu'), [])
        reference = normalize(other[0]['text']) if other else ''
        current = normalize(runs[0]['text'])
        fixture = cases[case]
        expected = fixture.get('expected_fragments',
                    ['مرحبابالعالم', 'simplesttreadsarabicandenglish'] if case.startswith(('arabic-', 'verified-arabic'))
                    else ['simplettsreadsthisscreenshot'])
        truth = normalize(fixture.get('truth', ''))
        summaries.append({'case': case, 'device': device, 'runs': len(runs),
                          'median_ms': (times[(len(times)-1)//2] + times[len(times)//2]) / 2,
                          'p90_ms': times[math.ceil(.9 * len(times)) - 1], 'min_ms': times[0],
                          'distinct_normalized_hashes': len({r['normalized_sha256'] for r in runs}),
                          'expected_fragments_present': all(fragment in current for fragment in expected),
                          'fixture_truth_similarity': difflib.SequenceMatcher(None, current, truth, autojunk=False).ratio() if truth else None,
                          # Repeated screenshot tiles are intentionally common;
                          # autojunk would discard their Arabic characters.
                          'other_device_similarity': difflib.SequenceMatcher(None, current, reference, autojunk=False).ratio() if other else None,
                          'pss_kib_max': max((r['pss_kib'] for r in runs if r['pss_kib'] is not None), default=None),
                          'rss_kib_max': max((r['rss_kib'] for r in runs if r['rss_kib'] is not None), default=None),
                          'gpu_mib_max': max((r['gpu_mib'] for r in runs if r['gpu_mib'] is not None), default=None)})
    return summaries


def write_results(results):
    results['summary'] = summarize(results)
    (OUTPUT / 'ocr-devices.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    if results['summary']:
        with (OUTPUT / 'ocr-devices.csv').open('w', newline='') as file:
            writer = csv.DictWriter(file, fieldnames=list(results['summary'][0]))
            writer.writeheader()
            writer.writerows(results['summary'])
    lines = ['OCR device benchmark', '',
             'Warm timings exclude image generation, memory sampling, and process startup.',
             'Cold time includes worker launch and first recognition. Missing GPU measurements are unknown.',
             'Text similarity compares outputs between devices; expected-fragment checks are limited and neither is an accuracy score.',
             f'Completed runs: {len(results["runs"])}. Error: {results.get("error") or "none"}', '']
    for block in results['blocks']:
        lines.append(f'Cold {block["device"]} block {block["block"]}: {block["cold_first_recognition_ms"]:.1f}ms, pid {block["pid"]}, memory {block["memory_after_cold"]}')
    lines.append('')
    for row in results['summary']:
        lines.append(f'{row["case"]:30} {row["device"]:3} n={row["runs"]} median={row["median_ms"]:.1f}ms p90={row["p90_ms"]:.1f}ms min={row["min_ms"]:.1f}ms PSS={row["pss_kib_max"]}KiB GPU={row["gpu_mib_max"]}MiB hashes={row["distinct_normalized_hashes"]} expected-fragments={row["expected_fragments_present"]} cross-device={row["other_device_similarity"]}')
    (OUTPUT / 'ocr-devices.txt').write_text('\n'.join(lines) + '\n')


def main():
    global OUTPUT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=OUTPUT)
    parser.add_argument('--case', action='append', default=[], help='run only a named case; repeat to select several')
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/release',
                        help='directory containing optimized reader-ocr (default: target/release)')
    parser.add_argument('--model', type=Path, default=ROOT / 'artifacts/combined-app/downloads/model-paddle')
    parser.add_argument('--cpu-runtime', type=Path, default=ROOT / 'artifacts/combined-app/memory/cpu-runtime')
    parser.add_argument('--gpu-runtime', type=Path, default=Path.home() / '.local/share/simple-tts/runtime')
    args = parser.parse_args()
    OUTPUT = args.output_dir.resolve()
    binary = args.bin_dir / 'reader-ocr'
    for path in (binary, args.model, args.cpu_runtime, args.gpu_runtime):
        if not path.exists():
            parser.error(f'missing input: {path}')
    if 'debug' in args.bin_dir.parts:
        parser.error('use an optimized release or extracted package binary, not target/debug')
    OUTPUT.mkdir(parents=True, exist_ok=True)
    results = {'method': {'order': ['cpu', 'gpu', 'gpu', 'cpu'], 'global_warmups': 3,
                          'shape_warmups_per_block': 1, 'runs_per_case_per_block': 3,
                          'timeout_seconds': 90, 'gpu_mode': 'strict'},
               'inputs': {key: str(value.resolve()) if isinstance(value, Path) else value
                          for key, value in vars(args).items()},
               'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'build_mode': 'optimized release/package (caller supplied)',
               'cases': [], 'blocks': [], 'runs': [], 'error': None}
    try:
        results['runtimes'] = {'cpu': ort_version(args.cpu_runtime.resolve()),
                               'gpu': ort_version(args.gpu_runtime.resolve())}
        if not results['runtimes']['cpu']['version'] or results['runtimes']['cpu']['version'] != results['runtimes']['gpu']['version']:
            raise RuntimeError('CPU and GPU ONNX Runtime versions differ or could not be verified')
        with tempfile.TemporaryDirectory(prefix='simple-stt-ocr-benchmark-') as temporary:
            scratch = Path(temporary).resolve()
            cases = create_cases(scratch)
            if args.case:
                cases = [case for case in cases if case['id'] in args.case]
                if {case['id'] for case in cases} != set(args.case):
                    raise ValueError('unknown benchmark case')
            fixture_output = OUTPUT / 'fixtures'
            fixture_output.mkdir(exist_ok=True)
            for case in cases:
                shutil.copyfile(case['path'], fixture_output / case['path'].name)
            results['cases'] = [{key: str(value) if isinstance(value, Path) else value
                                 for key, value in case.items() if key != 'path'} for case in cases]
            protocol = load_protocol()
            for index, device in enumerate(results['method']['order'], 1):
                run_block(protocol, device, index, (args.cpu_runtime if device == 'cpu' else args.gpu_runtime).resolve(),
                          args.model.resolve(), binary.resolve(), cases, scratch, results)
                write_results(results)
    except Exception as error:
        results['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        write_results(results)
    print(f'Results: {OUTPUT / "ocr-devices.txt"}')


if __name__ == '__main__':
    main()
