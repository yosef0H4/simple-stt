# Skill: add a desktop setting

Update all relevant locations:

1. `rust/src/config.rs`: add the serialized field, default, and validation.
2. `rust/src/gui.rs`: expose a native Win32 input control if user-facing.
3. `rust/src/app.rs`, `rust/src/audio.rs`, or `rust/src/worker.rs`: apply it.
4. Rust config round-trip tests.
5. `README.md` and `docs/ARCHITECTURE.md` when behavior changes.

Keep settings deterministic. Do not add anti-detection randomness.
