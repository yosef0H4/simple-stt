# Runtime memory residency

## Findings

- A warm GPU model necessarily retains the model/session resources it needs for reuse. Additional retained VRAM may be allocator reserve, CUDA context/library state or workspaces, not all active model weights. Without a breakdown, `nvidia-smi` process usage alone cannot attribute each byte. Multiple CUDA processes normally have separate contexts and context overhead; CUDA MPS changes scheduling/resource sharing but still isolates client address spaces and does not make separate model weights a shared allocation. It adds a desktop service and is not a suitable default here. [NVIDIA CUDA best practices](https://docs.nvidia.com/cuda/archive/12.2.0/cuda-c-best-practices-guide/index.html), [NVIDIA MPS](https://docs.nvidia.com/deploy/mps/when-to-use-mps.html).
- ONNX Runtime arenas normally retain freed blocks for reuse. Its optional arena shrinkage runs at the end of a `Run`, and can free unused arena regions; it does not promise to unload live session weights, CUDA contexts or library workspaces. The current Sherpa generation API does not expose per-run shrink options. Terminating the exact disposable worker is the clearest existing way to reclaim its resources after idle; measure actual post-exit GPU state because drivers and other processes also use memory. [ORT C documentation](https://onnxruntime.ai/docs/get-started/with-c.html).
- ORT CUDA `gpu_mem_limit` is an arena limit, not a cap on the entire process's VRAM. `arena_extend_strategy=kSameAsRequested` can limit oversizing relative to the default power-of-two growth, but neither setting guarantees a useful minimum working set or idle release. A too-low limit can make inference fail; choose values using real workloads rather than inventing a fixed budget for a 4 GB card. [ORT CUDA provider](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html).
- Paddle's current OAR/ORT worker passes `gpu_mem_limit: None` and `arena_extend_strategy: None`, so those provider options are available at its Rust integration point. Sherpa 1.13.8's [pinned CUDA session code](https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/csrc/session.cc) sets device and convolution search but does not forward arena limit/growth settings to CUDA. [Upstream issue and later change](https://github.com/k2-fsa/sherpa-onnx/issues/3982) describe adding that capability after 1.13.8. Do not assume a Sherpa config-file key changes CUDA arena behavior in the pinned runtime. It does forward `SessionConfig.`-prefixed ORT session keys; thread-spin options are session keys, not CUDA EP memory options.
- ORT worker threads can spin for responsiveness and consume CPU. `session.intra_op.allow_spinning=0` and `session.inter_op.allow_spinning=0` ask the runtime to sleep workers while idle, with a potential latency tradeoff. Pair this with existing condition-variable/deadline-driven supervision, and measure actual idle CPU: no general runtime guarantees precisely zero. [ORT threading](https://onnxruntime.ai/docs/performance/tune-performance/threading.html), [Sherpa 1.13.8 session parser](https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/sherpa-onnx/csrc/session.cc).
- Tesseract's official `tessdata_fast` provides Arabic and English integer LSTM models; its documentation describes a speed/accuracy tradeoff against `tessdata_best`. This establishes availability, not its accuracy or latency relative to the current bilingual Paddle model on this app's screenshots. [Tesseract model documentation](https://tesseract-ocr.github.io/tessdoc/Data-Files), [official fast models](https://github.com/tesseract-ocr/tessdata_fast).

## Smallest recommended policy

1. On a 4 GB GPU, measure GPU and CPU OCR on representative mixed Arabic/English screenshots before changing the default. Prefer CPU Paddle if it meets latency and recognition requirements; it avoids a separate OCR GPU context without introducing another engine. Consider Tesseract only if CPU Paddle is still too slow or heavy and the same image corpus demonstrates acceptable accuracy.
2. Keep TTS, OCR and STT disposable workers with independent demand-driven expiry. Avoid a second idle GPU TTS cache when simultaneous model/context residency exceeds the measured budget; retain at most what earns its memory through actual reuse. An idle GPU timeout should end the worker, not merely mark the model unloaded. Do not end an active worker without the existing cancellation and exact-PID rules.
3. Keep existing settings and backend choices rather than adding a broad allocator control panel. Investigate Paddle's CUDA arena growth policy and limits only with measured inputs and failure handling. The pinned Sherpa runtime needs a deliberate upstream update or custom build before its CUDA EP knobs can work; process lifecycle is available now.
4. Make warm workers sleep between requests and stop them by an event/deadline rather than periodic polling. Measure cold start, repeated reads, peak and idle RSS/VRAM, and post-exit VRAM on CPU and GPU separately. Treat the loaded model as an intentional warm cost, allocator slack as a separate optimization target, and unknown driver/context usage as an observation, not an assumed leak. These initial recommendations preceded the local measurements reported below.

## Smaller official Kokoro candidate

Sherpa publishes a Kokoro v1.0 INT8 bundle at [kokoro-int8-multi-lang-v1_0.tar.bz2](https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-int8-multi-lang-v1_0.tar.bz2). Its [release asset listing](https://github.com/k2-fsa/sherpa-onnx/releases/expanded_assets/tts-models) shows 126 MB and SHA-256 `72c347230107ef8a31098ec4d468a605244beef5f1ed00db97efa13518800653` for the October 26, 2026 upload. The filename is under a mutable release tag: verify the exact bytes and digest again before pinning. Separately, the maintainer's [Hugging Face model file](https://huggingface.co/csukuangfj/kokoro-int8-multi-lang-v1_0/blob/main/model.int8.onnx) is 114,202,993 bytes with SHA-256 `75635ea3b85a32d00094dcdd61a258aba87c160f7c264c304a7b3ae9d734195e`; that is the ONNX file digest, not the archive digest.

The [export workflow](https://github.com/k2-fsa/sherpa-onnx/blob/master/.github/workflows/export-kokoro.yaml) packages v1.0 `voices.bin`, `tokens.txt`, lexicons, eSpeak data and `model.int8.onnx` together. The current worker requires `model.onnx`, so this is not selectable without explicit loader/catalog changes. Its voice mapping should be validated against the exact pinned assets: the [maintainer's card](https://huggingface.co/csukuangfj/kokoro-int8-multi-lang-v1_0) states existing IDs 0–52 stay fixed in its 54-voice table. Sherpa 1.13.8 users report the variant loads, but a report of [NaN output on short texts](https://github.com/k2-fsa/sherpa-onnx/issues/4086) includes macOS ARM audible failures and Linux x64 internal NaNs; a separate [Android ARM report](https://github.com/k2-fsa/sherpa-onnx/issues/3754) describes clipped output. These are issue reports, not proof of the same observed failure on this machine. Do not replace the existing asset until repeated short and long English samples, finite/clipping checks and listening establish acceptable quality.

INT8 size does not establish retained PSS, cold start or warm speed. The user-provided baseline for the current model is about 516 MiB CPU PSS and 2.5 s warm inference versus 1.25 GiB PSS/652 MiB VRAM and 0.16 s warm on CUDA; compare the candidate under the same warm workload and idle budget before considering it. ONNX Runtime notes quantized CUDA operator support depends on operators and data types; an [ORT maintainer discussion](https://github.com/microsoft/onnxruntime/discussions/26735) specifically distinguishes supported MatMulInteger types. Check actual execution-provider assignment and CPU fallback, not only successful CUDA initialization. The [ORT quantization guide](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html) also notes potential accuracy and hardware tradeoffs.

The current [Supertonic 3 Sherpa archive](https://k2-fsa.github.io/sherpa/onnx/tts/supertonic.html) already uses INT8 for all four model components. Its catalog entry pins 128,774,318 bytes and SHA-256 `82fa96f91c4ef8abaae3a14a3f4153facf88bed821d1f7331cec2700f432c427`. I did not find a smaller official Sherpa Supertonic 3 archive in the release assets inspected; that is not proof none exists. No candidate was downloaded or benchmarked in this research.

## Windows memory probe

The shared idle policy can query the exact Windows worker PID with `PROCESS_QUERY_LIMITED_INFORMATION` and `K32GetProcessMemoryInfo`. It uses `PROCESS_MEMORY_COUNTERS_EX.PrivateUsage` (private committed bytes), falling back to the basic working set if extended counters are unavailable; a failed process open or query remains unknown and causes conservative eviction. Private commit and working set are not directly comparable to Linux PSS, and neither measures GPU allocations. [Microsoft documents the process access rights and counter layouts](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo); [PrivateUsage is commit charge](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex). GPU/automatic routes without a verified Windows VRAM measurement remain unknown and are conservatively evicted. Native Windows runtime and cross-build validation are still required.


## Local warm measurements and selected policy — 2026-11-04

These measurements use optimized Fedora binaries on this laptop (Ryzen 7 5800U,
RTX 3050 Ti 4 GB). Direct workers intentionally bypass production eviction to
measure honest warm speed and retained resources. No host audio, clipboard or
input was used. One native inference worker ran at a time, without a concurrent
build. Timings are not accuracy scores or promises for other hardware.

- STT: both current Q8 models, 1/5/15/30-second public audio, three warmups and
  five timed repetitions per case, CPU/Vulkan and CUDA GPU. For 30 seconds of
  audio the English model took 1.337 s CPU, 0.300 s Vulkan and 0.193 s CUDA;
  Arabic took 1.455 s CPU, 0.098 s Vulkan and 0.102 s CUDA. GPU remains useful.
  Vulkan is a compact option; CUDA's benefit varies by model and input.
- TTS: Kokoro English and Supertonic 3 Arabic, three text lengths, three warmups
  and five timed repetitions per case, CPU and CUDA on ONNX Runtime 1.28.2.
  Medium input took 3.111/0.187 s for Kokoro and 1.867/0.175 s for Supertonic
  (CPU/CUDA). CUDA is decisively faster on these warm voice workloads. The
  longest Kokoro case retained about 1172 MiB VRAM and 1.32 GiB PSS; Supertonic
  reached 626 MiB VRAM and 1.08 GiB PSS. CPU also retained 592/321 MiB PSS.
- OCR: a counterbalanced CPU/GPU/GPU/CPU order, three global warmups, a shape
  warmup and three timed runs per case per block (six per device). Two verified
  screenshots cover original and sparse/dense 640×240 through 4K shapes. Eight
  independent small public samples add English, Arabic, mixed scripts, code-like
  prose, four font sizes and light/dark backgrounds. CPU is the automatic choice
  for interactive selections: the first seven independent samples took about
  50–125 ms CPU versus 189–197 ms GPU, with matching normalized fixture text.
  Dense English 4K still benefits from explicit GPU: 2.704 s CPU versus 1.184 s
  GPU. CPU retained about 70–130 MiB PSS across this corpus and needs no VRAM.

The original OCR configuration retained up to 2450 MiB VRAM after a dense 4K
image, and kept that high-water allocation for later small selections. Paddle now
uses SameAsRequested arena growth, per-run arena shrinkage, and the bounded cuDNN
workspace option. With these changes the expanded corpus peaked at **206 MiB
post-run retained VRAM**, while the dense-4K warm median remained about 1.18 s.
This is a post-run measurement, not a global VRAM limit or an active peak bound.
The CUDA worker still retained about 946 MiB PSS, so process retirement remains
necessary. The provider arena is not given an arbitrary hard cap that could
silently exclude larger valid images. CPU OCR disables ORT thread spinning.

The shared idle policy samples a completed worker once per use. Retention needs
known RAM at most 1 GiB and known VRAM at most 768 MiB. Unknown measurements
retain the worker until its configured timeout. The earlier 256/128 MiB limits
evicted ordinary speech workers after every dictation and defeated warm reuse.
These revised bounds concern idle workers, not active
inference. Oversized workers are reaped after completion/drained playback, while
cheap CPU OCR may remain warm until its own timeout. Prewarm overlaps recording,
text acquisition and screen selection, and is scoped to the current intent.
Timeouts are maximums. The tradeoff is explicit: large models may reload on the
next action instead of reserving their measured working sets while inactive.
Last successful TTS route metadata survives restart; model processes do not.

Raw JSON/CSV, worker logs, fixtures and comparison text remain under ignored
`artifacts/combined-app/benchmarks/`. The repetitive tiled-image comparison uses
SequenceMatcher with autojunk disabled: default autojunk misleadingly reported
3% agreement for repeated Arabic. There are still real recognition differences
on downsampled dense Arabic; matching a phrase somewhere is not an accuracy
assessment. The independent mixed-script fixture uses separate Arabic/Latin
fonts so a missing Latin glyph cannot masquerade as an OCR failure. Human TTS
pronunciation acceptance and native Windows runtime validation remain separate.

## Current lifetime policy (2026-11-05)

The fixed estimate thresholds described in the earlier investigation no longer drive ordinary-completion eviction. The user prioritizes warm reuse after normal completion. STT, voice, API voice and OCR now honor configured independent idle deadlines; explicit unload, cancellation, shutdown and actual system memory pressure retain their earlier release paths. Unexpired voice language caches remain reusable until their own deadline.
