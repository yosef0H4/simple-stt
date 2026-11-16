#!/usr/bin/env python3
"""Real Paddle Arabic long-line regression, without desktop, audio or clipboard access."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unicodedata

from PIL import Image, ImageDraw, ImageFont, features

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('worker_test', ROOT / 'scripts/test-reader-workers.py')
workers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workers)


def normalized(text):
    text = unicodedata.normalize('NFKD', text)
    return ''.join(c for c in text if c.isalnum() and not unicodedata.combining(c)).replace('ى', 'ي')


def error_rate(reference, actual):
    reference, actual = normalized(reference), normalized(actual)
    row = list(range(len(actual) + 1))
    for i, expected in enumerate(reference, 1):
        next_row = [i]
        for j, found in enumerate(actual, 1):
            next_row.append(min(row[j] + 1, next_row[-1] + 1, row[j - 1] + (expected != found)))
        row = next_row
    return row[-1] / max(1, len(reference))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/reader-ocr')
    parser.add_argument('--model', type=Path, default=ROOT / 'artifacts/combined-app/downloads/model-paddle')
    parser.add_argument('--runtime', type=Path, default=Path.home() / '.local/share/simple-tts/runtime')
    parser.add_argument('--device', choices=['gpu', 'cpu'], default='gpu')
    parser.add_argument('--screenshot', type=Path, help='Optional user-supplied Wikipedia screenshot; never uploaded')
    parser.add_argument('--artifacts', type=Path, default=ROOT / 'artifacts/arabic-ocr-regression')
    args = parser.parse_args()
    assert features.check_feature('raqm'), 'Pillow needs Raqm to generate shaped Arabic fixtures'
    args.artifacts.mkdir(parents=True, exist_ok=True)
    cases = [
        ('long-22', 22, ['تساعد هذه الأداة على قراءة النص العربي من الشاشة بسرعة وسهولة، مع الحفاظ على ترتيب الكلمات والأرقام.']),
        ('long-28', 28, ['يستخدم القارئ نموذج التعرف على الحروف لاستخراج النص من الصور، ثم يرسل الكلمات إلى محرك الصوت المحلي.']),
        ('multiline', 23, ['تعمل هذه الأداة على قراءة النصوص العربية من الشاشة مع الحفاظ على ترتيب الكلمات في كل سطر.',
                           'يمكن للمستخدم اختيار اللغة والصوت المناسبين، ثم نسخ النص أو الاستماع إليه دون اتصال بالإنترنت.']),
        ('short', 28, ['مرحبا بالعالم هذا اختبار باللغة العربية.']),
        ('numbers', 22, ['يتحدث العربية أكثر من 467 مليون نسمة وتعترف بها 27 دولة لغة رسمية.']),
    ]
    with tempfile.TemporaryDirectory(prefix='simple-stt-arabic-ocr-') as temporary:
        scratch = Path(temporary)
        env = dict(os.environ, XDG_DATA_HOME=temporary, XDG_CACHE_HOME=temporary,
                   SIMPLE_TTS_CAPTURE_DIR=temporary, WAYLAND_DISPLAY='unavailable-test-display',
                   DISPLAY='', DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-test-bus',
                   LD_LIBRARY_PATH=str(args.runtime / 'lib') + ':' + os.environ.get('LD_LIBRARY_PATH', ''))
        fixtures = []
        for name, size, lines in cases:
            font = ImageFont.truetype(str(ROOT / 'assets/fonts/NotoSansArabic-Regular.ttf'), size)
            widths = [ImageDraw.Draw(Image.new('RGB', (1, 1))).textlength(line, font=font, direction='rtl') for line in lines]
            width = int(max(widths)) + 40
            image = Image.new('RGB', (width, len(lines) * (size * 2) + 30), 'white')
            draw = ImageDraw.Draw(image)
            for index, line in enumerate(lines):
                draw.text((width - 20, 10 + index * size * 2), line, font=font, fill='#141414', direction='rtl', language='ar', anchor='rt')
            path = scratch / f'{name}.png'
            image.save(path)
            fixtures.append((name, path, '\n'.join(lines)))
        if args.screenshot:
            import shutil
            path = scratch / 'wikipedia.png'
            shutil.copyfile(args.screenshot, path)
            fixtures.append(('wikipedia', path, None))
        report = []
        with (args.artifacts / 'arabic-worker.log').open('wb') as log:
            child, events = workers.launch(args.binary, ['--model-dir', str(args.model.resolve()), '--runtime-dir', str(args.runtime.resolve()), '--device', args.device], env, log)
            try:
                for name, path, expected in fixtures:
                    start = time.monotonic()
                    workers.send(child, {'type': 'recognize', 'job_id': name, 'generation': 1, 'image_path': str(path), 'language': 'ar'})
                    kind, event, _ = events.get(timeout=90)
                    assert kind == 'control' and event['type'] == 'text', event
                    assert events.get(timeout=10)[1]['type'] == 'finished'
                    text = event['text']
                    rate = error_rate(expected, text) if expected else None
                    report.append({'case': name, 'seconds': time.monotonic() - start, 'cer': rate, 'text': text})
                    # Modern small text must remain readable; tolerates model-specific
                    # diacritics and occasional spelling errors, not collapsed lines.
                    if rate is not None:
                        assert rate < .12, (name, rate, text)
                    if name == 'numbers':
                        assert '467' in text and '27' in text, text
                    if name == 'wikipedia':
                        compact = normalized(text)
                        for phrase in ['أكثر اللغات السامية تحدثا', 'أكثر اللغات انتشارا في العالم', '467', '27', 'الدول التي تعترف بها كلغة رسمية']:
                            assert normalized(phrase) in compact, (phrase, text)
                    print(f'PASS {name}: {rate if rate is not None else "exact phrases"}, {report[-1]["seconds"]:.2f}s', flush=True)
            finally:
                (args.artifacts / 'arabic-results.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
                workers.finish(child)


if __name__ == '__main__':
    main()
