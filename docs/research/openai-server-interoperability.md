# OpenAI-compatible server/client E2E

2026-11-15. Uses official `openai` Python SDK 3.51.0, local HTTP loopback, installed real Kokoro/Paddle/Parakeet models, and private headless host/client profiles. No cloud inference or user settings changes.

`python scripts/test-openai-server-e2e.py` exercises:

- SDK model discovery; invalid Bearer token produces AuthenticationError and unsupported speech model produces BadRequestError.
- Non-silent WAV speech; repeated and concurrent SDK requests reuse one native speech worker.
- Real image OCR through Chat Completions and Responses API.
- Multipart WAV transcription; repeated requests reuse the same STT worker.
- Our actual CLI/controller/remote-worker clients call our server for TTS, OCR and STT with client local fallback disabled and local TTS/OCR model paths absent.
- A disconnected STT endpoint fails when fallback is disabled and transcribes through the selected local engine when enabled.
- All model processes exit after the configured idle interval; the next STT request loads a new worker and returns the same fixture transcript.
- A timed-out SDK caller leaves a responsive server whose workers retire normally.

The idle check failed on the previous implementation: the persistent STT bridge blocked reading stdin and never ran the supervisor idle policy. The correction reads requests on a bounded channel and periodically runs the existing supervisor policy while idle. Warm reuse, exact worker process exit, and lazy reload then passed.

These are headless single-host network interoperability tests, not a two-physical-device or cloud-provider claim. The official SDK supports custom base URLs and normal API error parsing ([upstream](https://github.com/openai/openai-python)). Native desktop-priority scheduling, platform support, and every third-party provider are not established by this test alone.
