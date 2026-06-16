# Third-party notices

Simple STT source does not vendor Parakeet runtime binaries, GGUF model files, AutoHotkey, or other external source trees. Third-party components used by developers or included in a separate packaged build remain under their upstream licenses.

## Optional packaged runtime components

- AutoHotkey v2 runtime, used for the compiled desktop shell executable, when included in a packaged build.
- Parakeet runtime files under `runtime/external/parakeet-runtime/`, when included in a packaged build.
- Parakeet/GGUF speech model files under `runtime/external/parakeet-runtime/.../models/`, when included in a packaged build.
- Rust crate dependencies compiled into the Simple STT binaries, as recorded in `Cargo.lock`.

See the upstream projects and bundled files for detailed license terms. Do not publish packaged runtime/model artifacts unless you have reviewed and satisfied the applicable upstream redistribution and attribution requirements.
