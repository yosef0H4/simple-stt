use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CONFIG_SCHEMA_VERSION: u32 = 2;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapsLockBehavior {
    #[default]
    PreserveTap,
    AlwaysOff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextDeliveryMode {
    Type,
    #[default]
    PasteCtrlV,
    PasteCtrlShiftV,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub hotkey_enabled: bool,
    pub record_hotkey: String,
    pub toggle_delivery_hotkey: String,
    pub capslock_behavior: CapsLockBehavior,
    pub audio_device_contains: String,
    pub audio_gain: f32,
    pub typing_chunk_chars: usize,
    pub typing_interval_ms: u64,
    pub trailing_space: bool,
    pub text_delivery_mode: TextDeliveryMode,
    pub remove_punctuation: bool,
    pub lowercase_output: bool,
    pub idle_worker_timeout_secs: u64,
    pub worker_shutdown_grace_ms: u64,
    pub start_with_windows: bool,
    pub log_level: LogLevel,
    pub diagnostic_overlay: bool,
    pub log_transcripts: bool,
    pub inference_device: InferenceDevice,
    pub ui_theme: UiTheme,
    pub parakeet_runtime_dir: String,
    pub model_dir: String,
    pub selected_model_filename: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            hotkey_enabled: true,
            record_hotkey: "CapsLock+S".to_owned(),
            toggle_delivery_hotkey: "CapsLock+A".to_owned(),
            capslock_behavior: CapsLockBehavior::PreserveTap,
            audio_device_contains: String::new(),
            audio_gain: 1.0,
            typing_chunk_chars: 3,
            typing_interval_ms: 20,
            trailing_space: true,
            text_delivery_mode: TextDeliveryMode::PasteCtrlV,
            remove_punctuation: false,
            lowercase_output: false,
            idle_worker_timeout_secs: 180,
            worker_shutdown_grace_ms: 2_000,
            start_with_windows: false,
            log_level: LogLevel::Normal,
            diagnostic_overlay: false,
            log_transcripts: false,
            inference_device: InferenceDevice::Auto,
            ui_theme: UiTheme::Auto,
            parakeet_runtime_dir: r"external\parakeet-runtime\parakeet-windows-cuda".to_owned(),
            model_dir: r"external\parakeet-runtime\parakeet-windows-cuda\models".to_owned(),
            selected_model_filename: "tdt_ctc-110m-f16.gguf".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct LegacyConfig {
    idle_timeout_secs: Option<u64>,
    typing_interval_ms: Option<u64>,
    typing_chunk_chars: Option<usize>,
    audio_gain: Option<f32>,
    audio_device_contains: Option<String>,
    parakeet_runtime_dir: Option<String>,
    parakeet_model_path: Option<String>,
    start_with_windows: Option<bool>,
    hotkey_enabled: Option<bool>,
    record_hotkey: Option<String>,
    capslock_always_off: Option<bool>,
    log_level: Option<LogLevel>,
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
            !self.record_hotkey.trim().is_empty(),
            "record_hotkey must not be empty"
        );
        anyhow::ensure!(
            !self.toggle_delivery_hotkey.trim().is_empty(),
            "toggle_delivery_hotkey must not be empty"
        );
        anyhow::ensure!(
            self.audio_gain > 0.0 && self.audio_gain <= 10.0,
            "audio_gain must be in (0, 10]"
        );
        anyhow::ensure!(
            self.typing_chunk_chars > 0 && self.typing_chunk_chars <= 256,
            "typing_chunk_chars must be in [1, 256]"
        );
        anyhow::ensure!(
            self.typing_interval_ms <= 1_000,
            "typing_interval_ms must be <= 1000"
        );
        anyhow::ensure!(
            self.idle_worker_timeout_secs > 0,
            "idle_worker_timeout_secs must be positive"
        );
        anyhow::ensure!(
            (250..=30_000).contains(&self.worker_shutdown_grace_ms),
            "worker_shutdown_grace_ms must be in [250, 30000]"
        );
        anyhow::ensure!(
            !self.parakeet_runtime_dir.trim().is_empty(),
            "parakeet_runtime_dir must not be empty"
        );
        anyhow::ensure!(
            !self.model_dir.trim().is_empty(),
            "model_dir must not be empty"
        );
        validate_model_filename(&self.selected_model_filename)?;
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
        let schema_version = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| value.get("schema_version").and_then(|value| value.as_u64()));
        if schema_version == Some(CONFIG_SCHEMA_VERSION as u64) {
            let value: Self = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()))?;
            value.validate()?;
            return Ok(value);
        }
        let migrated = Self::migrate_legacy(&raw)?;
        let backup = path.with_extension("json.schema1.bak");
        if !backup.exists() {
            fs::copy(path, &backup)
                .with_context(|| format!("backing up old config to {}", backup.display()))?;
        }
        migrated.save_to(path)?;
        Ok(migrated)
    }

    fn migrate_legacy(raw: &str) -> Result<Self> {
        let old: LegacyConfig =
            serde_json::from_str(raw).context("parsing legacy schema-1 config")?;
        let mut value = Self::default();
        if let Some(v) = old.idle_timeout_secs {
            value.idle_worker_timeout_secs = v;
        }
        if let Some(v) = old.typing_interval_ms {
            value.typing_interval_ms = v;
        }
        if let Some(v) = old.typing_chunk_chars {
            value.typing_chunk_chars = v;
        }
        if let Some(v) = old.audio_gain {
            value.audio_gain = v;
        }
        if let Some(v) = old.audio_device_contains {
            value.audio_device_contains = v;
        }
        if let Some(v) = old.parakeet_runtime_dir {
            value.parakeet_runtime_dir = v;
        }
        if let Some(v) = old.parakeet_model_path {
            let path = PathBuf::from(v);
            if let Some(parent) = path.parent() {
                value.model_dir = parent.to_string_lossy().into_owned();
            }
            if let Some(file) = path.file_name() {
                value.selected_model_filename = file.to_string_lossy().into_owned();
            }
        }
        if let Some(v) = old.start_with_windows {
            value.start_with_windows = v;
        }
        if let Some(v) = old.hotkey_enabled {
            value.hotkey_enabled = v;
        }
        if let Some(v) = old.record_hotkey {
            value.record_hotkey = normalize_legacy_hotkey(&v);
        }
        if old.capslock_always_off.unwrap_or(false) {
            value.capslock_behavior = CapsLockBehavior::AlwaysOff;
        }
        if let Some(v) = old.log_level {
            value.log_level = v;
        }
        value.validate()?;
        Ok(value)
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
        self.resolve_from_runtime_root(&self.parakeet_runtime_dir)
    }
    pub fn model_dir_path(&self) -> PathBuf {
        self.resolve_from_runtime_root(&self.model_dir)
    }
    pub fn selected_model_path(&self) -> PathBuf {
        self.model_dir_path().join(&self.selected_model_filename)
    }
    pub fn validate_parakeet_files(&self) -> Result<()> {
        let runtime = self.parakeet_runtime_dir_path();
        let dll = runtime.join("bin").join("parakeet.dll");
        let model = self.selected_model_path();
        anyhow::ensure!(dll.exists(), "Parakeet DLL is missing: {}", dll.display());
        anyhow::ensure!(
            model.exists(),
            "Parakeet GGUF model is missing: {}",
            model.display()
        );
        Ok(())
    }
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

fn normalize_legacy_hotkey(value: &str) -> String {
    value
        .split('+')
        .map(|part| {
            let lower = part.trim().to_ascii_lowercase();
            match lower.as_str() {
                "capslock" | "caps" | "caps_lock" => "CapsLock".to_owned(),
                "ctrl" | "control" => "Ctrl".to_owned(),
                "alt" => "Alt".to_owned(),
                "shift" => "Shift".to_owned(),
                "win" | "windows" => "Win".to_owned(),
                _ if lower.len() == 1 => lower.to_ascii_uppercase(),
                _ => part.trim().to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
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
        assert_eq!(config.text_delivery_mode, TextDeliveryMode::PasteCtrlV);
        assert_eq!(config.toggle_delivery_hotkey, "CapsLock+A");
        assert!(!config.remove_punctuation);
        assert!(!config.lowercase_output);
        assert_eq!(config.inference_device, InferenceDevice::Auto);
    }

    #[test]
    fn schema2_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let config = AppConfig {
            typing_interval_ms: 33,
            ..Default::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(AppConfig::load_from(&path).unwrap(), config);
    }

    #[test]
    fn schema1_is_migrated_and_backed_up() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"idle_timeout_secs":45,"typing_interval_ms":12,"typing_chunk_chars":4,"audio_gain":1.5,"audio_device_contains":"Mic","parakeet_runtime_dir":"runtime","parakeet_model_path":"models\\old.gguf","start_with_windows":true,"hotkey_enabled":true,"record_hotkey":"capslock+s","capslock_always_off":true,"log_level":"debug"}"#).unwrap();
        let config = AppConfig::load_from(&path).unwrap();
        assert_eq!(config.schema_version, 2);
        assert_eq!(config.idle_worker_timeout_secs, 45);
        assert_eq!(config.record_hotkey, "CapsLock+S");
        assert_eq!(config.capslock_behavior, CapsLockBehavior::AlwaysOff);
        assert!(path.with_extension("json.schema1.bak").exists());
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
