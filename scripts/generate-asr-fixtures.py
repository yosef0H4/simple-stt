"""Generate deterministic text prompts as 16 kHz mono WAV ASR fixtures.

Development dependency: edge-tts. The audio service can change its voice output;
the saved WAV files and manifest are the test inputs.
"""
import argparse
import asyncio
import json
import subprocess
from pathlib import Path

import edge_tts

PROMPTS = [
    ("arabic-sa", "ar-SA-ZariyahNeural", "مرحبا كيف حالك اليوم؟ أريد أن أكتب رسالة باللغة العربية."),
    ("arabic-gulf", "ar-AE-FatimaNeural", "السلام عليكم، بكرة عندنا اجتماع في المكتب الساعة العاشرة."),
    ("arabic-eg", "ar-EG-SalmaNeural", "أنا عايز أراجع الملف قبل ما أبعت الرسالة."),
    ("arabic-technical", "ar-SA-HamedNeural", "افتح تطبيق جيت هب ثم اكتب تحديث المشروع في الملف."),
    ("english", "en-US-AriaNeural", "Please open the project settings and save the new model."),
]


async def generate(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    available = {voice["ShortName"] for voice in await edge_tts.list_voices()}
    manifest = []
    for name, preferred, text in PROMPTS:
        locale = preferred[:5]
        voice = preferred if preferred in available else next(
            (candidate for candidate in sorted(available) if candidate.startswith(locale)), None
        )
        if voice is None:
            raise RuntimeError(f"No voice available for {locale}")
        media = output / f"{name}.mp3"
        wav = output / f"{name}.wav"
        for attempt in range(3):
            try:
                await edge_tts.Communicate(text, voice).save(str(media))
                if media.stat().st_size == 0:
                    raise RuntimeError("empty synthesized audio")
                break
            except Exception:
                if attempt == 2:
                    raise
                await asyncio.sleep(2 * (attempt + 1))
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(media), "-ar", "16000", "-ac", "1", str(wav)], check=True)
        if wav.stat().st_size <= 44:
            raise RuntimeError(f"empty WAV: {wav}")
        media.unlink()
        manifest.append({"file": wav.name, "voice": voice, "text": text})
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=Path("fixtures/asr"))
    asyncio.run(generate(parser.parse_args().output))
