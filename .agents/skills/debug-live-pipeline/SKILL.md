# Skill: debug the live pipeline

Use the layers in order. Do not debug everything at once.

```powershell
cargo test --workspace
cargo run -p uvox -- list-inputs
cargo run -p uvox -- record-test --seconds 5 --output recording-test.wav
cargo run -p uvox -- type-test "Uvox typing test."
$env:RUST_LOG="uvox=debug"
cargo run -p uvox -- run
```

Expected live state sequence:

```text
CapsLockDown
→ start retaining 20 ms frames
→ spawn worker if absent
→ hello token accepted
→ loading_model
→ ready
→ send start(session_id)
→ drain ring buffer
→ partial events
→ commit deltas
→ fixed-rate Unicode text
```

On CapsLockUp, confirm that Rust cancels the typist synchronously before sending Python cancel.
