// Geometry tests live beside the private capture module in
// `src/read_along_capture.rs`; `cargo test` runs them as readerd unit tests.
// Keep this integration target without importing the whole binary module,
// which would compile unrelated platform backends a second time.
