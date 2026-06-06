"""Idempotent download of a small public NeMo sample WAV."""

from __future__ import annotations

import shutil
import urllib.request
from pathlib import Path
from typing import Callable

from .audio import validate_nemotron_wav
from .constants import SAMPLE_FILENAME, SAMPLE_URL

DownloadFn = Callable[[str, Path], None]


def default_cache_dir() -> Path:
    return Path.home() / ".cache" / "uvox" / "samples"


def _download(url: str, destination: Path) -> None:
    with urllib.request.urlopen(url, timeout=60) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output)


def fetch_sample(
    cache_dir: Path | None = None,
    *,
    force: bool = False,
    download_fn: DownloadFn = _download,
) -> Path:
    """Fetch the sample once and validate its WAV format before returning it."""
    cache_dir = cache_dir or default_cache_dir()
    cache_dir.mkdir(parents=True, exist_ok=True)
    destination = cache_dir / SAMPLE_FILENAME
    if destination.exists() and not force:
        validate_nemotron_wav(destination)
        return destination

    temporary = destination.with_suffix(".wav.part")
    temporary.unlink(missing_ok=True)
    try:
        download_fn(SAMPLE_URL, temporary)
        validate_nemotron_wav(temporary)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)
    return destination
