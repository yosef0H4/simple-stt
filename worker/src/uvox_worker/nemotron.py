"""CUDA-only whole-file and stateful live Nemotron inference.

The stateful path is adapted from NVIDIA NeMo's Apache-2.0 licensed
Online_ASR_Microphone_Demo_Cache_Aware_Streaming notebook. See THIRD_PARTY_NOTICES.md.
"""

from __future__ import annotations

import copy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .constants import ALLOWED_LOOKAHEAD_MS, ENCODER_STEP_MS, MODEL_NAME, SAMPLE_RATE
from .cuda import require_cuda


def _text_from_output(output: Any) -> str:
    if hasattr(output, "text"):
        return str(output.text)
    return str(output)


def transcribe_file(path: Path, model_name: str = MODEL_NAME) -> str:
    """Load Nemotron on CUDA and transcribe a complete WAV file."""
    require_cuda()
    import nemo.collections.asr as nemo_asr

    model = nemo_asr.models.ASRModel.from_pretrained(model_name=model_name)
    model = model.cuda().eval()
    outputs = model.transcribe([str(path)])
    if not outputs:
        return ""
    return _text_from_output(outputs[0])


@dataclass(frozen=True)
class StreamingConfig:
    model_name: str = MODEL_NAME
    lookahead_ms: int = 80

    @property
    def right_context_frames(self) -> int:
        if self.lookahead_ms not in ALLOWED_LOOKAHEAD_MS:
            raise ValueError(f"lookahead_ms must be one of {ALLOWED_LOOKAHEAD_MS}")
        return self.lookahead_ms // ENCODER_STEP_MS

    @property
    def chunk_samples(self) -> int:
        return int(SAMPLE_RATE * (ENCODER_STEP_MS + self.lookahead_ms) / 1000)


class NemotronStreamingRecognizer:
    """A single-session stateful recognizer using NeMo cache-aware RNNT inference."""

    def __init__(self, config: StreamingConfig | None = None) -> None:
        self.config = config or StreamingConfig()
        self._load_model()
        self.reset()

    def _load_model(self) -> None:
        require_cuda()
        import torch
        import nemo.collections.asr as nemo_asr
        from nemo.collections.asr.models.ctc_bpe_models import EncDecCTCModelBPE
        from omegaconf import OmegaConf, open_dict

        self.torch = torch
        self.OmegaConf = OmegaConf
        self.EncDecCTCModelBPE = EncDecCTCModelBPE
        model = nemo_asr.models.ASRModel.from_pretrained(model_name=self.config.model_name)
        self.model = model.cuda().eval()

        left_context = int(self.model.encoder.att_context_size[0])
        self.model.encoder.set_default_att_context_size(
            [left_context, self.config.right_context_frames]
        )

        # Nemotron is RNNT. Older hybrid checkpoints also accept decoder_type.
        try:
            self.model.change_decoding_strategy(decoder_type="rnnt")
        except TypeError:
            pass

        decoding_cfg = self.model.cfg.decoding
        with open_dict(decoding_cfg):
            decoding_cfg.strategy = "greedy"
            decoding_cfg.preserve_alignments = False
            if hasattr(self.model, "joint"):
                decoding_cfg.greedy.max_symbols = 10
                decoding_cfg.fused_batch_size = -1
        self.model.change_decoding_strategy(decoding_cfg)
        self.preprocessor = self._init_preprocessor()

    def _init_preprocessor(self) -> Any:
        cfg = copy.deepcopy(self.model._cfg)
        self.OmegaConf.set_struct(cfg.preprocessor, False)
        cfg.preprocessor.dither = 0.0
        cfg.preprocessor.pad_to = 0
        cfg.preprocessor.normalize = "None"
        preprocessor = self.EncDecCTCModelBPE.from_config_dict(cfg.preprocessor)
        return preprocessor.to(self.model.device)

    def reset(self) -> None:
        torch = self.torch
        self.cache_last_channel, self.cache_last_time, self.cache_last_channel_len = (
            self.model.encoder.get_initial_cache_state(batch_size=1)
        )
        self.previous_hypotheses = None
        self.pred_out_stream = None
        self.pre_encode_cache_size = int(self.model.encoder.streaming_cfg.pre_encode_cache_size[1])
        num_channels = int(self.model.cfg.preprocessor.features)
        self.cache_pre_encode = torch.zeros(
            (1, num_channels, self.pre_encode_cache_size), device=self.model.device
        )

    def _preprocess_audio(self, audio: Any) -> tuple[Any, Any]:
        torch = self.torch
        signal = torch.from_numpy(audio).unsqueeze_(0).to(self.model.device)
        signal_len = torch.tensor([audio.shape[0]], device=self.model.device)
        return self.preprocessor(input_signal=signal, length=signal_len)

    @staticmethod
    def _extract_text(hypotheses: Any) -> str:
        if not hypotheses:
            return ""
        first = hypotheses[0]
        return _text_from_output(first)

    def transcribe_chunk(self, pcm16_bytes: bytes) -> str:
        """Transcribe exactly one model chunk and return the current cumulative hypothesis."""
        import numpy as np

        if len(pcm16_bytes) != self.config.chunk_samples * 2:
            raise ValueError(
                f"expected {self.config.chunk_samples * 2} PCM bytes, got {len(pcm16_bytes)}"
            )
        audio = np.frombuffer(pcm16_bytes, dtype="<i2").astype(np.float32) / 32768.0
        processed_signal, processed_len = self._preprocess_audio(audio)
        processed_signal = self.torch.cat([self.cache_pre_encode, processed_signal], dim=-1)
        processed_len += self.cache_pre_encode.shape[-1]
        self.cache_pre_encode = processed_signal[:, :, -self.pre_encode_cache_size :]

        with self.torch.inference_mode():
            (
                self.pred_out_stream,
                transcribed_texts,
                self.cache_last_channel,
                self.cache_last_time,
                self.cache_last_channel_len,
                self.previous_hypotheses,
            ) = self.model.conformer_stream_step(
                processed_signal=processed_signal,
                processed_signal_length=processed_len,
                cache_last_channel=self.cache_last_channel,
                cache_last_time=self.cache_last_time,
                cache_last_channel_len=self.cache_last_channel_len,
                keep_all_outputs=False,
                previous_hypotheses=self.previous_hypotheses,
                previous_pred_out=self.pred_out_stream,
                drop_extra_pre_encoded=None,
                return_transcription=True,
            )
        return self._extract_text(transcribed_texts)
