# Third-party notices

## NVIDIA NeMo notebook adaptation

`worker/src/uvox_worker/nemotron.py` adapts the stateful cache-aware streaming approach demonstrated in NVIDIA NeMo's `Online_ASR_Microphone_Demo_Cache_Aware_Streaming.ipynb` notebook:

- https://github.com/NVIDIA-NeMo/NeMo/blob/main/tutorials/asr/Online_ASR_Microphone_Demo_Cache_Aware_Streaming.ipynb

The upstream NeMo repository and notebook are Apache License 2.0 licensed. The adaptation keeps the same essential sequence: configure lookahead, initialize encoder caches, build a streaming preprocessor, retain pre-encode feature cache, and call `conformer_stream_step` for each non-overlapping chunk.

The project-level GPL-2.0-only license remains applicable to this repository's combined source distribution. Preserve this notice when redistributing modified versions.

## NVIDIA Nemotron model

The project downloads and runs `nvidia/nemotron-speech-streaming-en-0.6b` at runtime. Model usage is governed by NVIDIA's Open Model License Agreement, linked from the model card:

- https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b

The model weights are not bundled in this repository archive.

## Public sample audio

The CLI smoke test downloads the small NeMo sample file used in NVIDIA NeMo examples and discussions:

- https://dldata-public.s3.us-east-2.amazonaws.com/2086-149220-0033.wav

The sample is cached locally and is not bundled in this archive.
