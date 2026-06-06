"""Small audio helpers shared by CLI tests and the live worker."""

from __future__ import annotations

import wave
from dataclasses import dataclass
from pathlib import Path

from .constants import SAMPLE_RATE
from .errors import AudioFormatError


@dataclass(frozen=True)
class WavInfo:
    path: Path
    channels: int
    sample_rate: int
    sample_width_bytes: int
    frame_count: int

    @property
    def duration_seconds(self) -> float:
        return self.frame_count / self.sample_rate


def inspect_wav(path: Path) -> WavInfo:
    with wave.open(str(path), "rb") as wav:
        return WavInfo(
            path=path,
            channels=wav.getnchannels(),
            sample_rate=wav.getframerate(),
            sample_width_bytes=wav.getsampwidth(),
            frame_count=wav.getnframes(),
        )


def validate_nemotron_wav(path: Path) -> WavInfo:
    info = inspect_wav(path)
    if info.channels != 1:
        raise AudioFormatError(f"Expected mono WAV, got {info.channels} channels: {path}")
    if info.sample_rate != SAMPLE_RATE:
        raise AudioFormatError(
            f"Expected {SAMPLE_RATE} Hz WAV, got {info.sample_rate} Hz: {path}"
        )
    if info.sample_width_bytes != 2:
        raise AudioFormatError(
            f"Expected signed 16-bit PCM WAV, got {info.sample_width_bytes * 8}-bit samples: {path}"
        )
    return info


def read_pcm16(path: Path) -> bytes:
    validate_nemotron_wav(path)
    with wave.open(str(path), "rb") as wav:
        return wav.readframes(wav.getnframes())
