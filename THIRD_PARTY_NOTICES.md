# Third-party notices

Simple STT source does not vendor Parakeet runtime binaries, GGUF model files, AutoHotkey, or other external source trees. Third-party components used by developers or included in a separate packaged build remain under their upstream licenses.

## Optional packaged runtime components

- AutoHotkey v2 runtime, used for the compiled desktop shell executable, when included in a packaged build.
- Parakeet runtime files under `runtime/external/parakeet-runtime/`, when included in a packaged build.
- Parakeet/GGUF speech model files under `runtime/external/parakeet-runtime/.../models/`, when included in a packaged build.
- Rust crate dependencies compiled into the Simple STT binaries, as recorded in `Cargo.lock`.

The optional Arabic model `lemura-arabic-asr-lite-q8_0.gguf` is converted from
[`lemuralabs/lemura-arabic-asr-lite`](https://huggingface.co/lemuralabs/lemura-arabic-asr-lite)
by Lemura AI Labs. The source model is licensed under Creative Commons
Attribution 4.0 International (CC BY 4.0). The GGUF is a format and
quantization conversion; it retains the source model's license and attribution.
Conversion used the `mudler/parakeet.cpp` v0.5.0 converter.
The finished Q8 GGUF is distributed at
https://huggingface.co/yosef0H4/lemura-arabic-asr-lite-GGUF.

The English Q8 model comes from
[`mudler/parakeet-cpp-gguf`](https://huggingface.co/mudler/parakeet-cpp-gguf).
The Windows Vulkan runtime is the official `mudler/parakeet.cpp` v0.5.0
shared-library release.

The three `fixtures/asr/real-fleurs-ar-eg-*.wav` recordings are resampled
from the `ar_eg/test` split of [Google FLEURS](https://huggingface.co/datasets/google/fleurs),
licensed under CC BY 4.0. They remain test fixtures and are not training data.

See the upstream projects and bundled files for detailed license terms. Do not publish packaged runtime/model artifacts unless you have reviewed and satisfied the applicable upstream redistribution and attribution requirements.
# Phosphor Icons

The Settings UI includes a small bundled subset of Phosphor Icons Core 2.1.1.
Copyright (c) 2023 Phosphor Icons. Licensed under the MIT License.
https://github.com/phosphor-icons/core

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

# Bundled fonts

JetBrains Mono is used for waveform glyphs; Noto Sans Arabic is used for Arabic UI text. Both are licensed under SIL Open Font License 1.1. The font license texts are in `assets/fonts/LICENSE-JetBrainsMono.txt` and `assets/fonts/LICENSE-NotoSansArabic.txt`, and Windows packages include them in `licenses/`. Noto Sans Arabic's Regular face and browser WOFF2 are derived from the upstream variable face at weight 400.
