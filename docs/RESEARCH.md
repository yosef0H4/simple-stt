# Research Notes

The current accepted implementation is native Parakeet through `parakeet.dll` with a CUDA GGUF model.

Abandoned paths:

- Python NeMo/Nemotron live streaming;
- Windows Live Captions helpers;
- C# helper processes;
- live partial translation/transcription.

The retained workflow is hold-to-record and release-to-transcribe because it is faster and more accurate for this app than the earlier live transcription experiments.
