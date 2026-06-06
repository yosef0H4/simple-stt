# Research notes

These are the primary sources used when assembling the prototype.

## Nemotron model

- Model card: https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b/blob/main/README.md
- Current checkpoint note: the model card states that an updated checkpoint was released on March 12, 2026.
- Architecture: cache-aware FastConformer RNNT, 600M parameters.
- Runtime choices: 80 ms, 160 ms, 560 ms, and 1120 ms chunks.
- Input requirement: mono WAV.
- Official local load path: `nemo_asr.models.ASRModel.from_pretrained(model_name="nvidia/nemotron-speech-streaming-en-0.6b")`.

## Stateful NeMo streaming

- Official microphone notebook: https://github.com/NVIDIA-NeMo/NeMo/blob/main/tutorials/asr/Online_ASR_Microphone_Demo_Cache_Aware_Streaming.ipynb
- Official cache-aware simulator: https://github.com/NVIDIA-NeMo/NeMo/blob/main/examples/asr/asr_cache_aware_streaming/speech_to_text_cache_aware_streaming_infer.py
- NeMo streaming model docs: https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/asr/models.html

The worker implementation follows the microphone notebook's stateful `conformer_stream_step` approach rather than retranscribing a rolling WAV.

## CUDA-only validation

- PyTorch Windows install and verification: https://pytorch.org/get-started/locally/
- PyTorch documents `torch.cuda.is_available()` as the verification call for CUDA accessibility.

## uv and CUDA wheels

- uv PyTorch integration: https://docs.astral.sh/uv/guides/integration/pytorch/
- uv CLI `--torch-backend`: https://docs.astral.sh/uv/reference/cli/
- The `auto` backend attempts to select the appropriate PyTorch index from installed CUDA drivers.

## Rust desktop stack

- CPAL audio capture: https://docs.rs/cpal/latest/cpal/
- Rust Windows bindings: https://github.com/microsoft/windows-rs
- Native Windows GUI: https://docs.rs/crate/native-windows-gui/latest

## Windows input limitations

- Win32 `SendInput`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- Low-level keyboard hook: https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc

`SendInput` is subject to Windows integrity restrictions and is not guaranteed to work in every application.
