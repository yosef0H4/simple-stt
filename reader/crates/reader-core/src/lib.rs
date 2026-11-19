//! Dependency-light contracts shared by the controller and disposable workers.
//! This crate deliberately does not depend on audio, OCR, HTTP clients, or model runtimes.
pub mod config;
pub mod cuda;
pub mod generation;
pub mod language;
pub mod local_ipc;
pub mod native_paths;
pub mod paddle;
pub mod provider_url;
pub mod recorded_action;
pub mod request_budget;
pub mod residency;
pub mod secrets;
pub mod text;
pub mod text_prep;
pub mod wire;

pub use config::{
    AppConfig, ConfigStore, DevicePreference, OcrProvider, SelectionCopyOverride,
    SelectionCopyShortcut, SpeechProvider,
};
pub use generation::{GenerationGate, JobKey, WorkerIncarnation};

pub mod routing;

pub mod provider_error;
