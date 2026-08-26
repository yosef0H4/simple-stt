use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CONFIG_SCHEMA_VERSION: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Minimal,
    #[default]
    Normal,
    Debug,
    Extreme,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Normal => "normal",
            Self::Debug => "debug",
            Self::Extreme => "extreme",
        }
    }
    pub fn tracing_filter(&self) -> &'static str {
        match self {
            Self::Minimal => "simple-stt=warn",
            Self::Normal => "simple-stt=info",
            Self::Debug => "simple-stt=debug",
            Self::Extreme => "simple-stt=trace",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapsLockBehavior {
    #[default]
    PreserveTap,
    AlwaysOff,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextDeliveryMode {
    Type,
    #[default]
    SmartPaste,
    PasteShiftInsert,
    PasteCtrlV,
    PasteCtrlShiftV,
    Clipboard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinuxAutomationBackend {
    #[default]
    Auto,
    Native,
    Wtype,
    Ydotool,
    Xdotool,
    ClipboardOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxDeliveryChoice {
    pub backend: LinuxAutomationBackend,
    pub mode: TextDeliveryMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppDeliveryOverride {
    pub app_id: String,
    pub mode: TextDeliveryMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum InferenceDevice {
    Cpu,
    NvidiaGpu,
    #[default]
    Auto,
}

impl InferenceDevice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::NvidiaGpu => "nvidia_gpu",
            Self::Auto => "auto",
        }
    }

    pub fn effective(self) -> Self {
        match self {
            Self::Auto => auto_inference_device(),
            other => other,
        }
    }
}

pub fn auto_inference_device() -> InferenceDevice {
    static RESOLVED: std::sync::OnceLock<InferenceDevice> = std::sync::OnceLock::new();
    *RESOLVED.get_or_init(|| {
        if std::env::var("SIMPLE_STT_AUTO_INFERENCE_DEVICE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("cpu"))
        {
            return InferenceDevice::Cpu;
        }
        if std::env::var("SIMPLE_STT_AUTO_INFERENCE_DEVICE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("nvidia_gpu"))
        {
            return InferenceDevice::NvidiaGpu;
        }
        if nvidia_smi_has_usable_gpu() {
            InferenceDevice::NvidiaGpu
        } else {
            InferenceDevice::Cpu
        }
    })
}

fn nvidia_smi_has_usable_gpu() -> bool {
    let mut child = match std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2_500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let Ok(output) = child.wait_with_output() else {
                    return false;
                };
                if !status.success() {
                    return false;
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                return stdout
                    .lines()
                    .filter_map(|line| line.trim().parse::<u64>().ok())
                    .any(|free_mb| free_mb >= 1024);
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => return false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiTheme {
    Light,
    Dark,
    #[default]
    Auto,
}

impl UiTheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    #[default]
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub schema_version: u32,
    pub general: GeneralConfig,
    pub audio: AudioConfig,
    pub speech: SpeechConfig,
    pub output: OutputConfig,
    pub diagnostics: DiagnosticsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralConfig {
    pub enabled: bool,
    pub recording_mode: RecordingMode,
    pub record_hotkey: String,
    pub toggle_delivery_hotkey: String,
    pub cancel_hotkey: String,
    pub capslock_behavior: CapsLockBehavior,
    pub start_at_login: bool,
    pub ui_theme: UiTheme,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    pub preferred_device_id: String,
    pub gain: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeechConfig {
    pub inference_device: InferenceDevice,
    pub runtime_dir: String,
    pub model_dir: String,
    pub selected_model_filename: String,
    pub idle_worker_timeout_secs: u64,
    pub worker_shutdown_grace_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputConfig {
    pub delivery_mode: TextDeliveryMode,
    pub enabled_delivery_modes: Vec<TextDeliveryMode>,
    pub linux_automation_backend: LinuxAutomationBackend,
    pub linux_delivery_cycle: Vec<LinuxDeliveryChoice>,
    pub app_overrides: Vec<AppDeliveryOverride>,
    pub paced_typing_enabled: bool,
    pub typing_speed_wpm: u64,
    pub trailing_space: bool,
    pub remove_punctuation: bool,
    pub lowercase: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticsConfig {
    pub log_level: LogLevel,
    pub diagnostic_overlay: bool,
    pub log_transcripts: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            general: GeneralConfig {
                enabled: true,
                recording_mode: if cfg!(target_os = "linux") {
                    RecordingMode::Toggle
                } else {
                    RecordingMode::Hold
                },
                record_hotkey: "CapsLock+S".to_owned(),
                toggle_delivery_hotkey: "CapsLock+D".to_owned(),
                cancel_hotkey: "CapsLock+A".to_owned(),
                capslock_behavior: CapsLockBehavior::PreserveTap,
                start_at_login: false,
                ui_theme: UiTheme::Auto,
            },
            audio: AudioConfig {
                preferred_device_id: String::new(),
                gain: 1.0,
            },
            speech: SpeechConfig {
                inference_device: InferenceDevice::Auto,
                runtime_dir: default_parakeet_runtime_dir(),
                model_dir: default_model_dir(),
                selected_model_filename: "tdt_ctc-110m-f16.gguf".to_owned(),
                idle_worker_timeout_secs: 180,
                worker_shutdown_grace_ms: 2_000,
            },
            output: OutputConfig {
                delivery_mode: TextDeliveryMode::SmartPaste,
                enabled_delivery_modes: vec![TextDeliveryMode::SmartPaste, TextDeliveryMode::Type],
                linux_automation_backend: LinuxAutomationBackend::Auto,
                linux_delivery_cycle: vec![
                    LinuxDeliveryChoice {
                        backend: LinuxAutomationBackend::Auto,
                        mode: TextDeliveryMode::SmartPaste,
                    },
                    LinuxDeliveryChoice {
                        backend: LinuxAutomationBackend::Auto,
                        mode: TextDeliveryMode::Type,
                    },
                ],
                app_overrides: Vec::new(),
                paced_typing_enabled: true,
                typing_speed_wpm: 450,
                trailing_space: true,
                remove_punctuation: false,
                lowercase: false,
            },
            diagnostics: DiagnosticsConfig {
                log_level: LogLevel::Normal,
                diagnostic_overlay: false,
                log_transcripts: false,
            },
        }
    }
}

fn default_parakeet_runtime_dir() -> String {
    #[cfg(windows)]
    {
        r"external\parakeet-runtime\parakeet-windows-cuda".to_owned()
    }
    #[cfg(target_os = "linux")]
    {
        "external/parakeet-runtime/parakeet-linux".to_owned()
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        "external/parakeet-runtime/parakeet-native".to_owned()
    }
}

fn default_model_dir() -> String {
    #[cfg(windows)]
    {
        r"external\parakeet-runtime\parakeet-windows-cuda\models".to_owned()
    }
    #[cfg(target_os = "linux")]
    {
        "external/parakeet-runtime/models".to_owned()
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        "external/parakeet-runtime/models".to_owned()
    }
}

pub fn parakeet_native_library_candidates(runtime: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        candidates.push(runtime.join("bin").join("parakeet.dll"));
        candidates.push(runtime.join("parakeet.dll"));
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(runtime.join("bin").join("libparakeet.so"));
        candidates.push(runtime.join("lib").join("libparakeet.so"));
        candidates.push(runtime.join("libparakeet.so"));
        candidates.push(runtime.join("bin").join("parakeet.so"));
        candidates.push(runtime.join("lib").join("parakeet.so"));
        candidates.push(runtime.join("parakeet.so"));
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push(runtime.join("bin").join("libparakeet.dylib"));
        candidates.push(runtime.join("lib").join("libparakeet.dylib"));
        candidates.push(runtime.join("libparakeet.dylib"));
    }
    candidates
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == CONFIG_SCHEMA_VERSION,
            "unsupported config schema_version {}; expected {}",
            self.schema_version,
            CONFIG_SCHEMA_VERSION
        );
        anyhow::ensure!(
            !self.general.record_hotkey.trim().is_empty(),
            "record_hotkey must not be empty"
        );
        anyhow::ensure!(
            !self.general.toggle_delivery_hotkey.trim().is_empty(),
            "toggle_delivery_hotkey must not be empty"
        );
        anyhow::ensure!(
            !self.general.cancel_hotkey.trim().is_empty(),
            "cancel_hotkey must not be empty"
        );
        anyhow::ensure!(
            self.audio.gain > 0.0 && self.audio.gain <= 10.0,
            "audio_gain must be in (0, 10]"
        );
        anyhow::ensure!(
            (50..=450).contains(&self.output.typing_speed_wpm),
            "typing_speed_wpm must be in [50, 450]"
        );
        anyhow::ensure!(
            !self.output.enabled_delivery_modes.is_empty(),
            "enabled_delivery_modes must not be empty"
        );
        anyhow::ensure!(
            self.output
                .enabled_delivery_modes
                .contains(&self.output.delivery_mode),
            "delivery_mode must be included in enabled_delivery_modes"
        );
        for app_override in &self.output.app_overrides {
            anyhow::ensure!(
                !app_override.app_id.trim().is_empty() && app_override.app_id.len() <= 160,
                "Linux app override IDs must contain 1 to 160 characters"
            );
        }
        anyhow::ensure!(
            self.speech.idle_worker_timeout_secs > 0,
            "idle_worker_timeout_secs must be positive"
        );
        anyhow::ensure!(
            (250..=30_000).contains(&self.speech.worker_shutdown_grace_ms),
            "worker_shutdown_grace_ms must be in [250, 30000]"
        );
        anyhow::ensure!(
            !self.speech.runtime_dir.trim().is_empty(),
            "parakeet_runtime_dir must not be empty"
        );
        anyhow::ensure!(
            !self.speech.model_dir.trim().is_empty(),
            "model_dir must not be empty"
        );
        validate_model_filename(&self.speech.selected_model_filename)?;
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        if let Some(path) = std::env::var_os("SIMPLE_STT_CONFIG") {
            return PathBuf::from(path);
        }
        instance_config_dir().join("config.json")
    }

    pub fn local_data_dir() -> PathBuf {
        instance_local_data_dir()
    }

    pub fn logs_dir() -> PathBuf {
        Self::local_data_dir().join("logs")
    }
    pub fn state_dir() -> PathBuf {
        Self::local_data_dir().join("state")
    }
    pub fn shell_log_path() -> PathBuf {
        Self::logs_dir().join("simple-stt-shell.log")
    }
    pub fn capture_log_path() -> PathBuf {
        Self::logs_dir().join("simple-stt-capture.log")
    }
    pub fn infer_log_path() -> PathBuf {
        Self::logs_dir().join("simple-stt-infer.log")
    }
    pub fn service_state_path() -> PathBuf {
        Self::state_dir().join("capture-state.json")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            let value = Self::default();
            value.save_to(path)?;
            return Ok(value);
        }
        let raw =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        let value = Self::normalize_json(&parsed);
        value.save_to(path)?;
        Ok(value)
    }

    pub fn normalize_json(input: &serde_json::Value) -> Self {
        let defaults = Self::default();
        let object = input.as_object();
        let section = |name: &str| object.and_then(|value| value.get(name));
        let mut normalized = Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            general: normalize_section(&defaults.general, section("general")),
            audio: normalize_section(&defaults.audio, section("audio")),
            speech: normalize_section(&defaults.speech, section("speech")),
            output: normalize_section(&defaults.output, section("output")),
            diagnostics: normalize_section(&defaults.diagnostics, section("diagnostics")),
        };
        if normalized.general.record_hotkey.trim().is_empty() {
            normalized.general.record_hotkey = defaults.general.record_hotkey;
        }
        if normalized.general.toggle_delivery_hotkey.trim().is_empty() {
            normalized.general.toggle_delivery_hotkey = defaults.general.toggle_delivery_hotkey;
        }
        if normalized.general.cancel_hotkey.trim().is_empty() {
            normalized.general.cancel_hotkey = defaults.general.cancel_hotkey;
        }
        if !(normalized.audio.gain > 0.0 && normalized.audio.gain <= 10.0) {
            normalized.audio.gain = defaults.audio.gain;
        }
        if !(50..=450).contains(&normalized.output.typing_speed_wpm) {
            normalized.output.typing_speed_wpm = defaults.output.typing_speed_wpm;
        }
        normalized.output.enabled_delivery_modes.dedup();
        if normalized.output.enabled_delivery_modes.is_empty() {
            normalized.output.enabled_delivery_modes = defaults.output.enabled_delivery_modes;
        }
        if !normalized
            .output
            .enabled_delivery_modes
            .contains(&normalized.output.delivery_mode)
        {
            normalized
                .output
                .enabled_delivery_modes
                .push(normalized.output.delivery_mode);
        }
        normalized
            .output
            .app_overrides
            .retain(|entry| !entry.app_id.trim().is_empty() && entry.app_id.len() <= 160);
        for entry in &mut normalized.output.app_overrides {
            entry.app_id = entry.app_id.trim().to_owned();
        }
        normalized
            .output
            .app_overrides
            .dedup_by(|left, right| left.app_id.eq_ignore_ascii_case(&right.app_id));
        if normalized.speech.idle_worker_timeout_secs == 0 {
            normalized.speech.idle_worker_timeout_secs = defaults.speech.idle_worker_timeout_secs;
        }
        if !(250..=30_000).contains(&normalized.speech.worker_shutdown_grace_ms) {
            normalized.speech.worker_shutdown_grace_ms = defaults.speech.worker_shutdown_grace_ms;
        }
        if normalized.speech.runtime_dir.trim().is_empty() {
            normalized.speech.runtime_dir = defaults.speech.runtime_dir;
        }
        if normalized.speech.model_dir.trim().is_empty() {
            normalized.speech.model_dir = defaults.speech.model_dir;
        }
        if validate_model_filename(&normalized.speech.selected_model_filename).is_err() {
            normalized.speech.selected_model_filename = defaults.speech.selected_model_filename;
        }
        normalized
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let temp = path.with_extension("json.tmp");
        {
            let mut file =
                fs::File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
            file.write_all((serde_json::to_string_pretty(self)? + "\n").as_bytes())?;
            file.flush()?;
            file.sync_all()?;
        }
        replace_file_atomic(&temp, path)
            .with_context(|| format!("atomically replacing {}", path.display()))
    }

    pub fn resolve_from_runtime_root(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            runtime_root().join(path)
        }
    }
    pub fn parakeet_runtime_dir_path(&self) -> PathBuf {
        self.resolve_from_runtime_root(&self.speech.runtime_dir)
    }
    pub fn model_dir_path(&self) -> PathBuf {
        self.resolve_from_runtime_root(&self.speech.model_dir)
    }
    pub fn selected_model_path(&self) -> PathBuf {
        self.model_dir_path()
            .join(&self.speech.selected_model_filename)
    }
    pub fn validate_parakeet_files(&self) -> Result<()> {
        let runtime = self.parakeet_runtime_dir_path();
        let library = parakeet_native_library_candidates(&runtime)
            .into_iter()
            .find(|path| path.exists());
        anyhow::ensure!(
            library.is_some(),
            "Parakeet native library is missing under {}",
            runtime.display()
        );
        let model = self.selected_model_path();
        anyhow::ensure!(
            model.exists(),
            "Parakeet GGUF model is missing: {}",
            model.display()
        );
        Ok(())
    }
}

fn normalize_section<T>(defaults: &T, input: Option<&serde_json::Value>) -> T
where
    T: Serialize + serde::de::DeserializeOwned + Clone,
{
    let mut accepted = serde_json::to_value(defaults).expect("default config section serializes");
    let Some(input) = input.and_then(serde_json::Value::as_object) else {
        return defaults.clone();
    };
    let Some(default_fields) = accepted.as_object() else {
        return defaults.clone();
    };
    let known_fields = default_fields.keys().cloned().collect::<Vec<_>>();
    for (key, candidate) in input {
        if !known_fields.iter().any(|known| known == key) {
            continue;
        }
        let previous = accepted
            .as_object_mut()
            .expect("config section remains an object")
            .insert(key.clone(), candidate.clone());
        if serde_json::from_value::<T>(accepted.clone()).is_err() {
            if let Some(previous) = previous {
                accepted
                    .as_object_mut()
                    .expect("config section remains an object")
                    .insert(key.clone(), previous);
            }
        }
    }
    serde_json::from_value(accepted).expect("normalized config section deserializes")
}

#[cfg(windows)]
pub fn replace_file_atomic(source: &Path, target: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source_wide: Vec<u16> = source
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    anyhow::ensure!(
        ok != 0,
        "MoveFileExW failed while replacing {}",
        target.display()
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn replace_file_atomic(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target)
        .with_context(|| format!("renaming {} to {}", source.display(), target.display()))
}

pub fn validate_model_filename(filename: &str) -> Result<()> {
    anyhow::ensure!(
        !filename.trim().is_empty(),
        "model filename must not be empty"
    );
    anyhow::ensure!(
        filename.ends_with(".gguf"),
        "model filename must end with .gguf"
    );
    anyhow::ensure!(
        !filename.contains('/') && !filename.contains('\\') && !filename.contains(".."),
        "model filename must be a plain approved filename"
    );
    Ok(())
}

/// Returns the runtime installation root for resolving relative configured paths.
///
/// During checkout development Cargo places binaries under `target\debug` or
/// `target\release`, so walk back to the checkout root. A staged distribution
/// places binaries directly beside the shell, so use the executable directory.
pub fn runtime_root() -> PathBuf {
    if let Some(path) = std::env::var_os("SIMPLE_STT_RUNTIME_ROOT") {
        return PathBuf::from(path);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            let profile = directory.file_name().and_then(|value| value.to_str());
            let parent_name = directory
                .parent()
                .and_then(|value| value.file_name())
                .and_then(|value| value.to_str());
            if matches!(profile, Some("debug" | "release")) && parent_name == Some("target") {
                if let Some(root) = directory.parent().and_then(Path::parent) {
                    return root.to_path_buf();
                }
            }
            return directory.to_path_buf();
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn app_instance_id() -> String {
    let root = runtime_root();
    let stem = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("simple-stt");
    let sanitized = stem
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in root.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!(
        "{}-{:016x}",
        if sanitized.is_empty() {
            "simple-stt"
        } else {
            &sanitized
        },
        hash
    )
}

fn instance_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("simple-stt")
        .join("instances")
        .join(app_instance_id())
}

fn instance_local_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("simple-stt")
        .join("instances")
        .join(app_instance_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = AppConfig::default();
        config.validate().unwrap();
        assert_eq!(config.output.delivery_mode, TextDeliveryMode::SmartPaste);
        assert_eq!(
            config.general.recording_mode,
            if cfg!(target_os = "linux") {
                RecordingMode::Toggle
            } else {
                RecordingMode::Hold
            }
        );
        assert_eq!(config.general.toggle_delivery_hotkey, "CapsLock+D");
        assert_eq!(config.general.cancel_hotkey, "CapsLock+A");
        assert!(!config.output.remove_punctuation);
        assert!(!config.output.lowercase);
        assert_eq!(config.speech.inference_device, InferenceDevice::Auto);
    }

    #[test]
    fn schema5_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut config = AppConfig::default();
        config.output.typing_speed_wpm = 72;
        config.save_to(&path).unwrap();
        assert_eq!(AppConfig::load_from(&path).unwrap(), config);
    }

    #[test]
    fn app_delivery_overrides_normalize_and_validate() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["output"]["app_overrides"] = serde_json::json!([
            {"app_id":"  kitty  ","mode":"paste_ctrl_shift_v"},
            {"app_id":"ExampleGame.exe","mode":"type"},
            {"app_id":"","mode":"clipboard"}
        ]);
        let config = AppConfig::normalize_json(&value);
        assert_eq!(config.output.app_overrides.len(), 2);
        assert_eq!(config.output.app_overrides[0].app_id, "kitty");
        assert_eq!(config.output.app_overrides[1].mode, TextDeliveryMode::Type);
        config.validate().unwrap();
    }

    #[test]
    fn partial_config_is_normalized_and_unknown_fields_are_removed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"general":{"recording_mode":"toggle","typo":true},"audio":{"gain":2.5},"obsolete":1}"#).unwrap();
        let config = AppConfig::load_from(&path).unwrap();
        assert_eq!(config.general.recording_mode, RecordingMode::Toggle);
        assert_eq!(config.audio.gain, 2.5);
        let normalized = fs::read_to_string(path).unwrap();
        assert!(!normalized.contains("typo"));
        assert!(!normalized.contains("obsolete"));
        assert!(normalized.contains("selected_model_filename"));
    }

    #[test]
    fn invalid_field_defaults_without_losing_valid_siblings() {
        let value = serde_json::json!({"audio":{"gain":"loud","preferred_device_id":"mic-1"}});
        let config = AppConfig::normalize_json(&value);
        assert_eq!(config.audio.gain, 1.0);
        assert_eq!(config.audio.preferred_device_id, "mic-1");
        let out_of_range = AppConfig::normalize_json(
            &serde_json::json!({"audio":{"gain":99.0,"preferred_device_id":"mic-2"}}),
        );
        assert_eq!(out_of_range.audio.gain, 1.0);
        assert_eq!(out_of_range.audio.preferred_device_id, "mic-2");
    }

    #[test]
    fn malformed_json_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, "{broken").unwrap();
        assert!(AppConfig::load_from(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
    }

    #[test]
    fn traversal_model_filename_is_rejected() {
        assert!(validate_model_filename("..\\bad.gguf").is_err());
    }

    #[test]
    fn absolute_runtime_path_is_not_rebased() {
        let config = AppConfig::default();
        let absolute = if cfg!(windows) {
            PathBuf::from(r"C:\simple-stt\runtime")
        } else {
            PathBuf::from("/opt/simple-stt/runtime")
        };
        assert_eq!(
            config.resolve_from_runtime_root(absolute.to_str().unwrap()),
            absolute
        );
    }

    #[test]
    fn instance_paths_are_scoped_by_runtime_root() {
        let original = std::env::var_os("SIMPLE_STT_RUNTIME_ROOT");
        let temp_a = tempfile::tempdir().unwrap();
        let temp_b = tempfile::tempdir().unwrap();
        std::env::set_var("SIMPLE_STT_RUNTIME_ROOT", temp_a.path());
        let config_a = AppConfig::config_path();
        let data_a = AppConfig::local_data_dir();
        std::env::set_var("SIMPLE_STT_RUNTIME_ROOT", temp_b.path());
        let config_b = AppConfig::config_path();
        let data_b = AppConfig::local_data_dir();
        match original {
            Some(value) => std::env::set_var("SIMPLE_STT_RUNTIME_ROOT", value),
            None => std::env::remove_var("SIMPLE_STT_RUNTIME_ROOT"),
        }
        assert_ne!(config_a, config_b);
        assert_ne!(data_a, data_b);
        assert!(config_a.to_string_lossy().contains("instances"));
        assert!(data_a.to_string_lossy().contains("instances"));
    }
}
