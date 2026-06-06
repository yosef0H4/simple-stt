import wave
from pathlib import Path

from uvox_worker.sample import fetch_sample


def write_valid_wav(path: Path):
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setframerate(16000)
        wav.setsampwidth(2)
        wav.writeframes(b"\0" * 320)


def test_fetch_sample_downloads_once_and_uses_cache(tmp_path):
    calls = []

    def fake_download(url, destination):
        calls.append(url)
        write_valid_wav(destination)

    first = fetch_sample(tmp_path, download_fn=fake_download)
    second = fetch_sample(tmp_path, download_fn=fake_download)
    assert first == second
    assert len(calls) == 1
