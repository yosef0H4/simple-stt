# Skill: add a Simple STT setting

Update the canonical schema-v2 path consistently:

1. `src/config.rs`: serialized field, default, validation, and schema-1 migration when relevant.
2. `src/bin/simple-stt-ctl.rs`: `config-show` and `config-save` translation.
3. `ahk/lib/Config.ahk`: persisted key list.
4. `ahk/lib/SettingsGui.ahk`: user-facing control when appropriate.
5. Capture/infer/shell owner: apply live, recycle worker, restart capture, or document shell restart.
6. Rust tests and `docs/configuration.md`.

Keep one JSON config file. Do not add undocumented sidecar state. Do not introduce clipboard-default typing or anti-detection randomness.
