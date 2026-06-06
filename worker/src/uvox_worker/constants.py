"""Project-wide worker constants."""

from __future__ import annotations

MODEL_NAME = "nvidia/nemotron-speech-streaming-en-0.6b"
SAMPLE_RATE = 16_000
SAMPLE_FILENAME = "2086-149220-0033.wav"
SAMPLE_URL = "https://dldata-public.s3.us-east-2.amazonaws.com/2086-149220-0033.wav"
EXPECTED_SAMPLE_WORDS = ("portrait", "phoebe")
DEFAULT_LOOKAHEAD_MS = 80
ALLOWED_LOOKAHEAD_MS = (0, 80, 480, 1040)
ENCODER_STEP_MS = 80
DEFAULT_STABILITY_OBSERVATIONS = 1
