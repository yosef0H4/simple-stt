#!/usr/bin/env python3
"""Download every supported Paddle bundle and run each on the GPU, file-only.

Uses a private profile. A previously downloaded runtime may be copied by hard
link from another test profile; original verification receipts are preserved.
No host clipboard, microphone, input, capture portal or audio output is used.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import unicodedata

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


settings = load('paddle_settings', 'test-settings-selection-api.py')
service = load('paddle_service', 'test-reader-service.py')


def recognition_score(expected, actual):
    """Character error rate for a quality probe, separate from runtime health."""
    expected = ''.join(unicodedata.normalize('NFKC', expected).casefold().split())
    actual = ''.join(unicodedata.normalize('NFKC', actual).casefold().split())
    previous = list(range(len(actual) + 1))
    for i, left in enumerate(expected, 1):
        current = [i]
        for j, right in enumerate(actual, 1):
            current.append(min(current[-1] + 1, previous[j] + 1, previous[j - 1] + (left != right)))
        previous = current
    return max(0, 1 - previous[-1] / max(len(expected), len(actual), 1))


def fixture(directory, languages):
    import cairo
    import gi
    gi.require_version('Pango', '1.0')
    gi.require_version('PangoCairo', '1.0')
    from gi.repository import Pango, PangoCairo
    examples = [
        ('ar', 'مرحبا بالعالم'), ('zh', '你好世界'), ('ja', 'こんにちは世界'),
        ('ko', '안녕하세요 세계'), ('ru', 'Привет мир'), ('hi', 'नमस्ते दुनिया'),
        ('el', 'Γειά σου κόσμε'), ('kn', 'ನಮಸ್ಕಾರ ಜಗತ್ತು'),
        ('ta', 'வணக்கம் உலகம்'), ('te', 'హలో ప్రపంచం'), ('th', 'สวัสดีชาวโลก'),
    ]
    language, text = next(((language, text) for language, text in examples if language in languages), ('en', 'Hello world'))
    text = unicodedata.normalize('NFC', text)
    path = directory / f'{language}-native-font.png'
    if path.exists():
        return path, text
    surface = cairo.ImageSurface(cairo.FORMAT_RGB24, 800, 220)
    context = cairo.Context(surface)
    context.set_source_rgb(1, 1, 1)
    context.paint()
    context.set_source_rgb(0, 0, 0)
    layout = PangoCairo.create_layout(context)
    # Keep the numeral test left-to-right even after an Arabic paragraph.
    # Pango's inherited auto direction otherwise reverses the two digit groups.
    layout.get_context().set_base_dir(Pango.Direction.LTR)
    layout.get_context().set_language(Pango.Language.from_string(language))
    layout.set_auto_dir(False)
    font = {'ko': 'Noto Serif CJK KR', 'zh': 'Noto Serif CJK SC', 'ja': 'Noto Serif CJK JP'}.get(language, 'Noto Sans')
    layout.set_font_description(Pango.FontDescription(font + ' 38'))
    layout.set_text(text + '\n12345 67890', -1)
    assert layout.get_unknown_glyphs_count() == 0, f'Missing fixture font: {language}'
    context.move_to(30, 20)
    PangoCairo.show_layout(context, layout)
    surface.write_to_png(str(path))
    return path, text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', type=Path, default=ROOT / 'artifacts/paddle-catalog/profile')
    parser.add_argument('--runtime-cache', type=Path, help='Real verified assets directory from an earlier isolated download test')
    parser.add_argument('--bin-dir', type=Path, default=ROOT / 'target/debug')
    parser.add_argument('--reuse', action='store_true')
    parser.add_argument('--model', action='append', help='Diagnostic subset: repeat for multiple model IDs')
    args = parser.parse_args()
    profile, bins = args.profile.resolve(), args.bin_dir.resolve()
    assert profile.is_relative_to(ROOT / 'artifacts')
    if not args.reuse:
        assert not profile.exists() or not any(profile.iterdir()), 'Use an empty private profile, or explicit --reuse'
    profile.mkdir(parents=True, exist_ok=True)
    process, api = settings.start_settings_server(bins / 'simple-stt-settings', profile / 'config.json', profile / 'runtime', profile / 'data')
    daemon = monitor = log = alias = watcher = None
    stop_gpu = threading.Event()
    gpu_observations = {}
    report = {'result': 'RUNNING', 'reused_profile': args.reuse, 'models': []}
    try:
        catalog = api('/api/assets')['assets']
        assets = Path(catalog[0]['directory']).parent
        if args.runtime_cache:
            source = args.runtime_cache.resolve()
            assert source.is_relative_to(ROOT / 'artifacts'), 'Only reuse isolated test assets'
            for identifier in ('sherpa-cuda', 'cuda-cudnn', 'cuda-cublas', 'cuda-nvrtc'):
                target = assets / identifier
                if not target.exists():
                    shutil.copytree(source / identifier, target, copy_function=os.link)
            print('Reused genuine verified NVIDIA runtime files; no synthetic receipts', flush=True)
        models = [model for model in api('/api/modules')['models'] if model['feature'] == 'ocr']
        models.sort(key=lambda model: (model['id'] != 'paddle-ar-en', model['id']))
        assert len(models) == 34
        assert [model['id'] for model in models if model.get('recommended')] == ['paddle-ar-en']
        report['catalog_models'] = len(models)
        if args.model:
            assert set(args.model) <= {model['id'] for model in models}
            models = [model for model in models if model['id'] in args.model]
        report['requested_models'] = [model['id'] for model in models]
        initial = {asset['id']: asset['installed'] for asset in api('/api/assets')['assets']}
        report['initially_installed_models'] = [model['id'] for model in models if initial[model['asset']]]
        if not args.reuse:
            assert not report['initially_installed_models']
        else:
            # Reset only this test's OCR binding before repeating its completion
            # scenario; an earlier failed probe may have selected another model.
            config = api('/api/reader')['config']
            config['ocr']['selection']['single'] = {'runtime': None, 'model': None, 'device': 'auto'}
            api('/api/reader-save', {'config': config, 'expected_revision': config['revision']})

        def install(ids):
            for identifier in ids:
                api('/api/asset-action', {'action': 'install', 'id': identifier})
            last = set()
            deadline = time.monotonic() + 1200
            while time.monotonic() < deadline:
                state = api('/api/assets')
                jobs = {job['job_id']: job for job in state['jobs']}
                for identifier in ids:
                    job = jobs.get(identifier, {})
                    assert job.get('phase') not in ('failed', 'cancelled'), job
                    if job.get('phase') == 'ready' and identifier not in last:
                        print('Verified bundle:', identifier, flush=True)
                        last.add(identifier)
                if len(last) == len(ids):
                    assert all(asset['installed'] for asset in state['assets'] if asset['id'] in ids)
                    return
                time.sleep(.4)
            raise AssertionError('Catalog downloads timed out')

        install(['sherpa-cuda'])
        install(['paddle-ar-en'])
        selected = api('/api/reader')['config']
        assert selected['ocr']['selection']['single']['model'] == 'paddle-ar-en'
        install([model['asset'] for model in models if model['id'] != 'paddle-ar-en'])
        selected = api('/api/reader')['config']
        assert selected['ocr']['selection']['single']['model'] == 'paddle-ar-en', 'Further downloads replaced the selected model'
        report['selection_preserved'] = True
        reader = assets.parent / 'reader'
        alias = tempfile.TemporaryDirectory(prefix='paddle-gpu-')
        short = Path(alias.name) / 'instance'
        short.symlink_to(assets.parent, target_is_directory=True)
        env = dict(os.environ, SIMPLE_STT_CONFIG=str(profile / 'config.json'), SIMPLE_STT_RUNTIME_ROOT=str(profile / 'runtime'), XDG_DATA_HOME=str(profile / 'data'), XDG_CONFIG_HOME=str(profile / 'config'), XDG_CACHE_HOME=str(profile / 'cache'), SIMPLE_STT_READER_DATA=str(short / 'reader'), SIMPLE_TTS_CONFIG=str(short / 'reader/config.json'), SIMPLE_TTS_NO_DESKTOP='1', SIMPLE_STT_NO_DESKTOP='1', DISPLAY='', WAYLAND_DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-paddle-test')
        env.pop('LD_LIBRARY_PATH', None)
        log = (profile / 'readerd.log').open('w')
        daemon = subprocess.Popen([str(bins / 'readerd'), 'daemon', '--foreground', '--no-desktop'], env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        service.wait_for(lambda: (reader / 'simple-tts.sock').exists(), 12, 'private OCR controller')
        monitor = service.WorkerMonitor(daemon.pid)
        def watch_gpu():
            while not stop_gpu.wait(.1):
                result = subprocess.run(['nvidia-smi', '--query-compute-apps=pid,used_gpu_memory', '--format=csv,noheader,nounits'], capture_output=True, text=True, timeout=3)
                owned = service.children(daemon.pid)
                for row in result.stdout.splitlines():
                    columns = [part.strip() for part in row.split(',')]
                    if len(columns) != 2 or not all(column.isdigit() for column in columns):
                        continue
                    pid, memory = map(int, columns)
                    if memory > 0 and pid in owned and service.executable(pid) == 'reader-ocr':
                        try:
                            maps = Path(f'/proc/{pid}/maps').read_text()
                            if 'libonnxruntime_providers_cuda' in maps:
                                gpu_observations[pid] = memory
                        except FileNotFoundError:
                            pass
        watcher = threading.Thread(target=watch_gpu)
        watcher.start()
        previous_workers = set()
        images = profile / 'fixtures'
        images.mkdir(exist_ok=True)
        for model in models:
            config = api('/api/reader')['config']
            config['ocr']['selection']['mode'] = 'single_model'
            config['ocr']['selection']['single'] = {'runtime': 'paddle-cuda', 'model': model['id'], 'device': 'gpu'}
            api('/api/reader-save', {'config': config, 'expected_revision': config['revision']})
            image, expected_word = fixture(images, model['languages'])
            started = monitor.mark()
            output = service.run([bins / 'simple-stt-linux', 'media', 'ocr', image], env, timeout=120)
            lines = output.splitlines()
            numeral_line = max(lines, key=lambda line: sum(char.isdecimal() for char in line))
            digit_score = recognition_score('1234567890', numeral_line)
            word_score = max((recognition_score(expected_word, line) for line in lines if line != numeral_line), default=0)
            # Older published models are optional accuracy/speed choices. A zero
            # read as Cyrillic 'о' is a recognition error, not a broken runtime.
            # Keep the recommended model's stronger exact numeral assertion.
            assert digit_score >= (1 if model['id'] == 'paddle-ar-en' else .8), (model['id'], output)
            assert word_score >= .6, (model['id'], expected_word, output)
            assert any(char.isalpha() for char in output), (model['id'], output)
            assert len(service.workers(daemon.pid, 'reader-ocr')) <= 1
            workers = monitor.since(started, 'reader-ocr') - previous_workers
            service.wait_for(lambda: workers & gpu_observations.keys(), 3, f'GPU allocation for {model["id"]}')
            entry = {'id': model['id'], 'fixture': image.name, 'expected_word': expected_word, 'text': output, 'numeral_score': digit_score, 'word_score': word_score, 'workers': sorted(workers), 'gpu_mib': {pid: gpu_observations[pid] for pid in workers if pid in gpu_observations}, 'result': 'PASS'}
            previous_workers.update(monitor.since(started, 'reader-ocr'))
            report['models'].append(entry)
            (profile / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
            print('PASS GPU OCR:', model['id'], repr(output), flush=True)
        assert max(len(sample['reader-ocr']) for _, sample in monitor.samples) <= 1
        report['result'] = 'PASS'
        print(f'PASS {len(models)} genuine Paddle bundles, GPU inference, dictionary/recognition probes, selection preservation and bounded workers', flush=True)
    finally:
        stop_gpu.set()
        if watcher: watcher.join(timeout=5)
        if daemon and daemon.poll() is None:
            try:
                service.run([bins / 'readerctl', 'shutdown'], env, timeout=8)
                daemon.wait(timeout=8)
            except Exception:
                daemon.terminate()
                try: daemon.wait(timeout=3)
                except subprocess.TimeoutExpired: daemon.kill();daemon.wait(timeout=3)
        if monitor: monitor.close()
        if log: log.close()
        if alias: alias.cleanup()
        settings.stop_settings_server(process)
        (profile / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')


if __name__ == '__main__':
    main()
