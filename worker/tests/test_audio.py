import wave
from pathlib import Path

import pytest

from uvox_worker.audio import validate_nemotron_wav
from uvox_worker.errors import AudioFormatError


def write_wav(path: Path, *, channels=1, rate=16000, width=2, frames=160):
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setframerate(rate)
        wav.setsampwidth(width)
        wav.writeframes(b"\0" * frames * channels * width)


def test_validate_wav_accepts_16khz_mono_pcm16(tmp_path):
    path = tmp_path / "ok.wav"
    write_wav(path)
    info = validate_nemotron_wav(path)
    assert info.sample_rate == 16000
    assert info.channels == 1
    assert info.duration_seconds == pytest.approx(0.01)

@pytest.mark.parametrize(
    ("kwargs", "message"),
    [
        ({"channels": 2}, "mono"),
        ({"rate": 8000}, "16000"),
        ({"width": 1}, "16-bit"),
    ],
)
def test_validate_wav_rejects_wrong_format(tmp_path, kwargs, message):
    path = tmp_path / "bad.wav"
    write_wav(path, **kwargs)
    with pytest.raises(AudioFormatError, match=message):
        validate_nemotron_wav(path)
