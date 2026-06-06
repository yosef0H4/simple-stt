"""CUDA-only whole-file NVIDIA Parakeet inference via NeMo/PyTorch."""

from __future__ import annotations

from pathlib import Path
import tempfile
from typing import Any

import numpy as np
import soundfile as sf

from .constants import SAMPLE_RATE
from .cuda import require_cuda

MODEL_NAME = "nvidia/parakeet-tdt_ctc-110m"


def _text_from_output(output: Any) -> str:
    if hasattr(output, "text"):
        return str(output.text)
    return str(output)


def transcribe_file(path: Path, model_name: str = MODEL_NAME) -> str:
    """Load Parakeet on CUDA and transcribe one complete audio file."""
    require_cuda()
    import nemo.collections.asr as nemo_asr

    model = nemo_asr.models.ASRModel.from_pretrained(model_name=model_name)
    model = model.cuda().eval()
    outputs = model.transcribe([str(path)])
    if not outputs:
        return ""
    return _text_from_output(outputs[0]).strip()


class ParakeetFileRecognizer:
    """Persistent CUDA Parakeet recognizer for complete push-to-talk recordings."""

    def __init__(self, model_name: str = MODEL_NAME) -> None:
        require_cuda()
        import nemo.collections.asr as nemo_asr

        model = nemo_asr.models.ASRModel.from_pretrained(model_name=model_name)
        self.model = model.cuda().eval()

    def transcribe_pcm16(self, pcm16_bytes: bytes, sample_rate: int = SAMPLE_RATE) -> str:
        if len(pcm16_bytes) % 2:
            raise ValueError("PCM16 recording contains an odd number of bytes")
        audio = np.frombuffer(pcm16_bytes, dtype="<i2").astype(np.float32) / 32768.0
        if audio.size == 0:
            return ""
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as handle:
            path = Path(handle.name)
        try:
            sf.write(path, audio, sample_rate, subtype="PCM_16")
            outputs = self.model.transcribe([str(path)])
        finally:
            path.unlink(missing_ok=True)
        if not outputs:
            return ""
        return _text_from_output(outputs[0]).strip()
