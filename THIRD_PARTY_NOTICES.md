# Third-party notices

## Native Parakeet runtime

Uvox dynamically loads a local native Parakeet CUDA runtime from:

```text
external\parakeet-runtime\parakeet-windows-cuda
```

That runtime bundle is a local artifact and is intentionally not committed to git. Preserve the runtime's own license and notice files when redistributing it.

## NVIDIA Parakeet model

The default model path is:

```text
external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf
```

Model usage is governed by the model publisher's terms. The model file is not committed to this repository.

## Public sample audio

The retained smoke-test fixture is based on the public LibriSpeech sample:

```text
tests\fixtures\parakeet-smoke.wav
```

Expected transcript anchor:

```text
Well, I don't wish to see it any more
```
