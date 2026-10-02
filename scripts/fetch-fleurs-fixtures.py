"""Fetch three CC BY 4.0 Arabic human speech clips from Google FLEURS."""
import json
import subprocess
import urllib.request
from pathlib import Path

API = "https://datasets-server.huggingface.co/first-rows?dataset=google/fleurs&config=ar_eg&split=test"
SOURCE = "https://huggingface.co/datasets/google/fleurs"


def main() -> None:
    output = Path("fixtures/asr")
    output.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(API, timeout=30) as response:
        rows = json.load(response)["rows"][:3]
    manifest_path = output / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest = [entry for entry in manifest if not entry["file"].startswith("real-fleurs-")]
    for index, entry in enumerate(rows):
        name = f"real-fleurs-ar-eg-{index:03}.wav"
        source_audio = output / f"{name}.source.wav"
        wav = output / name
        urllib.request.urlretrieve(entry["row"]["audio"][0]["src"], source_audio)
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source_audio), "-ar", "16000", "-ac", "1", str(wav)], check=True)
        source_audio.unlink()
        manifest.append({"file": name, "text": entry["row"]["transcription"],
                         "kind": "human", "source": SOURCE, "license": "CC BY 4.0"})
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
