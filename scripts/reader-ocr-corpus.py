#!/usr/bin/env python3
"""Generate a private, synthetic Arabic/English OCR corpus under artifacts/.

Requires Pillow built with libraqm (HarfBuzz and FriBidi). This script performs
no OCR, downloads, screenshots, or accuracy assertions. Each invocation needs
an empty output directory and writes PNGs plus manifest.json.
"""
import argparse
import json
import shutil
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, features

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_FONT = ROOT / 'assets/fonts/NotoSansArabic-Regular.ttf'

# Original examples authored for this corpus. The expected strings use logical
# Unicode order; Pillow/RAQM handles shaping and bidirectional presentation.
TEXTS = [
    ('arabic_short', 'ar', 'مرحباً بك في المكتبة'),
    ('arabic_long', 'ar', 'تفتح المكتبة أبوابها كل صباح وتوفر للزوار مكاناً هادئاً للقراءة والعمل'),
    ('mixed_digits', 'ar', 'وصل الطلب رقم 467 يوم 27 مايو، ورمز التتبع هو AB-2026.'),
    ('english', 'en', 'The meeting starts at 09:30 in room 27. Please bring the blue folder.'),
    ('paragraph', 'ar', 'افتتح المركز قاعة جديدة للقراءة هذا الأسبوع.\nتضم القاعة 467 كتاباً و27 مقعداً للزوار.\nيمكن استعارة الكتب من الساعة 9:00 حتى 17:00.'),
]
THEMES = {
    'light': ((248, 249, 247), (25, 28, 31)),
    'dark': ((27, 31, 38), (239, 241, 244)),
    'low_contrast_blue': ((204, 219, 230), (65, 92, 112)),
}
SIZES = (16, 22, 32)
MARGIN = 24


def requires_mixed_font(text):
    # Paragraphs with western digits also need complete Latin/digit coverage.
    return any(char.isascii() and char.isalnum() for char in text)


def system_mixed_font():
    if not shutil.which('fc-match'):
        raise SystemExit('fontconfig fc-match is required to locate DejaVu Sans; pass --mixed-font explicitly')
    result = subprocess.run(['fc-match', '--format', '%{file}', 'DejaVu Sans'],
                            check=True, capture_output=True, text=True, timeout=10)
    if not result.stdout.strip():
        raise SystemExit('could not locate DejaVu Sans; pass --mixed-font explicitly')
    return Path(result.stdout.strip())


def check_coverage(font, texts):
    if not shutil.which('fc-query'):
        raise SystemExit('fontconfig fc-query is required to check glyph coverage before rendering')
    result = subprocess.run(['fc-query', '--format', '%{charset}\n', str(font)],
                            check=True, capture_output=True, text=True, timeout=10)
    ranges = []
    try:
        for token in result.stdout.split():
            start, separator, end = token.partition('-')
            ranges.append((int(start, 16), int(end, 16) if separator else int(start, 16)))
    except ValueError as error:
        raise SystemExit(f'could not parse fontconfig coverage for {font}: {error}') from error
    if not ranges:
        raise SystemExit(f'fontconfig returned no character coverage for {font}')
    characters = {char for text in texts for char in text if not char.isspace()}
    missing = sorted(char for char in characters
                     if not any(start <= ord(char) <= end for start, end in ranges))
    if missing:
        summary = ', '.join(f'U+{ord(char):04X} ({char})' for char in missing)
        raise SystemExit(f'font {font} lacks required glyphs: {summary}')


def render(text, language, font_path, size, background, foreground, output):
    font = ImageFont.truetype(str(font_path), size, layout_engine=ImageFont.Layout.RAQM)
    direction = 'rtl' if language == 'ar' else 'ltr'
    # Measure every shaped line using exactly the same direction, language,
    # anchor and font as drawing. Right-align RTL lines within the content box.
    probe = ImageDraw.Draw(Image.new('RGB', (1, 1), background))
    lines = text.split('\n')
    bounds = [probe.textbbox((0, 0), line, font=font, anchor='lt',
                             direction=direction, language=language) for line in lines]
    widths = [right - left for left, top, right, bottom in bounds]
    heights = [bottom - top for left, top, right, bottom in bounds]
    if any(width <= 0 or height <= 0 for width, height in zip(widths, heights)):
        raise ValueError('fixture contains an empty or unrenderable line')
    max_width = max(widths)
    gap = max(8, size // 2)
    image = Image.new('RGB', (2 * MARGIN + max_width,
                               2 * MARGIN + sum(heights) + gap * (len(lines) - 1)), background)
    draw = ImageDraw.Draw(image)
    y = MARGIN
    for line, (left, top, right, bottom), width, height in zip(lines, bounds, widths, heights):
        x = MARGIN + (max_width - width if direction == 'rtl' else 0) - left
        draw.text((x, y - top), line, font=font, fill=foreground, anchor='lt',
                  direction=direction, language=language)
        y += height + gap
    image.save(output, format='PNG')
    return {'width': image.width, 'height': image.height}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output_dir', type=Path, help='empty directory beneath this checkout’s artifacts/')
    parser.add_argument('--font', type=Path, default=DEFAULT_FONT,
                        help='font file; default is bundled Noto Sans Arabic (Noto Naskh Arabic or DejaVu Sans may be supplied)')
    parser.add_argument('--mixed-font', type=Path, help='full Arabic and Latin font; defaults to system DejaVu Sans via fontconfig')
    args = parser.parse_args()
    if not features.check('raqm'):
        raise SystemExit('Pillow with libraqm is required for Arabic shaping and mixed-direction layout')
    font = args.font.expanduser().resolve()
    if not font.is_file():
        raise SystemExit(f'font not found: {font}')
    mixed_font = (args.mixed_font or system_mixed_font()).expanduser().resolve()
    if not mixed_font.is_file():
        raise SystemExit(f'mixed font not found: {mixed_font}')
    output = args.output_dir.expanduser().resolve()
    artifacts = (ROOT / 'artifacts').resolve()
    if output == artifacts or not output.is_relative_to(artifacts):
        raise SystemExit('output directory must be a dedicated directory beneath artifacts/')
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        raise SystemExit('output directory must be empty')
    # Check every code point before creating any output; a font missing even
    # one required glyph must never silently produce tofu in a benchmark.
    check_coverage(font, [text for _, _, text in TEXTS if not requires_mixed_font(text)])
    check_coverage(mixed_font, [text for _, _, text in TEXTS if requires_mixed_font(text)])
    output.mkdir(parents=True, exist_ok=True)
    cases = []
    for size in SIZES:
        for theme_index, (theme, (background, foreground)) in enumerate(THEMES.items()):
            # Four of five texts per size/theme, rotating the omission so all
            # categories appear across sizes and backgrounds: 36 total cases.
            omitted = (SIZES.index(size) * len(THEMES) + theme_index) % len(TEXTS)
            for index, (kind, language, text) in enumerate(TEXTS):
                if index == omitted:
                    continue
                case_id = f'{kind}-{size}-{theme}'
                path = output / f'{case_id}.png'
                case_font = mixed_font if requires_mixed_font(text) else font
                dimensions = render(text, language, case_font, size, background, foreground, path)
                cases.append({
                    'case_id': case_id,
                    'input_path': str(path),
                    'expected_text': text,
                    'conditions': {'text_kind': kind, 'language': language, 'font_size': size,
                                   'font_path': str(case_font), 'theme': theme,
                                   'background_rgb': list(background), 'foreground_rgb': list(foreground),
                                   'direction': 'rtl' if language == 'ar' else 'ltr', **dimensions},
                })
    (output / 'manifest.json').write_text(json.dumps({'version': 1, 'cases': cases}, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'Wrote {len(cases)} fixtures and manifest to {output}')


if __name__ == '__main__':
    main()
