use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Minimal,
    Normal,
    Debug,
    Extreme,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Minimal
    }
}

impl LogLevel {
    pub fn tracing_filter(&self) -> &'static str {
        match self {
            Self::Minimal => "uvox=warn",
            Self::Normal => "uvox=info",
            Self::Debug => "uvox=debug",
            Self::Extreme => "uvox=trace",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub idle_timeout_secs: u64,
    pub typing_interval_ms: u64,
    pub typing_chunk_chars: usize,
    pub audio_gain: f32,
    pub audio_device_contains: String,
    pub parakeet_runtime_dir: String,
    pub parakeet_model_path: String,
    pub start_with_windows: bool,
    pub hotkey_enabled: bool,
    pub record_hotkey: String,
    pub capslock_always_off: bool,
    pub log_level: LogLevel,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 180,
            typing_interval_ms: 20,
            typing_chunk_chars: 3,
            audio_gain: 1.0,
            audio_device_contains: String::new(),
            parakeet_runtime_dir: r"external\parakeet-runtime\parakeet-windows-cuda".to_owned(),
            parakeet_model_path:
                r"external\parakeet-runtime\parakeet-windows-cuda\models\tdt_ctc-110m-f16.gguf"
                    .to_owned(),
            start_with_windows: false,
            hotkey_enabled: true,
            record_hotkey: "capslock+s".to_owned(),
            capslock_always_off: false,
            log_level: LogLevel::Minimal,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.idle_timeout_secs > 0,
            "idle_timeout_secs must be positive"
        );
        anyhow::ensure!(
            self.typing_interval_ms <= 1_000,
            "typing_interval_ms must be <= 1000"
        );
        anyhow::ensure!(
            self.typing_chunk_chars > 0,
            "typing_chunk_chars must be positive"
        );
        anyhow::ensure!(
            self.audio_gain > 0.0 && self.audio_gain <= 10.0,
            "audio_gain must be in (0, 10]"
        );
        anyhow::ensure!(
            !self.parakeet_runtime_dir.trim().is_empty(),
            "parakeet_runtime_dir must not be empty"
        );
        anyhow::ensure!(
            !self.parakeet_model_path.trim().is_empty(),
            "parakeet_model_path must not be empty"
        );
        crate::hotkey::HotkeySpec::parse(&self.record_hotkey)?;
        Ok(())
    }

    pub fn validate_parakeet_files(&self) -> Result<()> {
        let runtime = self.parakeet_runtime_dir_path();
        anyhow::ensure!(
            runtime.exists(),
            "Parakeet runtime is missing: {}",
            runtime.display()
        );
        let bin = runtime.join("bin");
        anyhow::ensure!(
            bin.exists(),
            "Parakeet bin directory is missing: {}",
            bin.display()
        );
        let dll = bin.join("parakeet.dll");
        anyhow::ensure!(dll.exists(), "Parakeet DLL is missing: {}", dll.display());
        let model = self.parakeet_model_path();
        anyhow::ensure!(
            model.exists(),
            "Parakeet GGUF model is missing: {}",
            model.display()
        );
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        if let Some(value) = std::env::var_os("UVOX_CONFIG") {
            return PathBuf::from(value);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("uvox")
            .join("config.json")
    }

    pub fn log_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("uvox")
            .join("latest.log")
    }

    pub fn model_store_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("uvox")
            .join("models")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            let value = Self::default();
            value.save_to(&path)?;
            return Ok(value);
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let value: Self =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        value.validate()?;
        if !raw.contains("\"record_hotkey\"") {
            value.save_to(path)?;
        }
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
        fs::write(path, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn resolve_from_repo(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            repo_root().join(path)
        }
    }

    pub fn parakeet_runtime_dir_path(&self) -> PathBuf {
        self.resolve_from_repo(&self.parakeet_runtime_dir)
    }

    pub fn parakeet_model_path(&self) -> PathBuf {
        self.resolve_from_repo(&self.parakeet_model_path)
    }
}

pub fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir.parent().unwrap_or(&manifest_dir);
    if candidate.join(".git").exists() || candidate.join("Cargo.lock").exists() {
        candidate.to_path_buf()
    } else {
        manifest_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn config_round_trip() {
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
    fn old_config_defaults_to_capslock_s_hotkey() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            r#"{
  "idle_timeout_secs": 180,
  "typing_interval_ms": 20,
  "typing_chunk_chars": 3,
  "audio_gain": 1.0,
  "audio_device_contains": "",
  "parakeet_runtime_dir": "external\\parakeet-runtime\\parakeet-windows-cuda",
  "parakeet_model_path": "external\\parakeet-runtime\\parakeet-windows-cuda\\models\\tdt_ctc-110m-f16.gguf",
  "start_with_windows": false,
  "hotkey_enabled": true,
  "capslock_always_off": false,
  "log_level": "minimal"
}"#,
        )
        .unwrap();
        let config = AppConfig::load_from(&path).unwrap();
        assert_eq!(config.record_hotkey, "capslock+s");
    }

    #[test]
    fn invalid_gain_is_rejected() {
        let config = AppConfig {
            audio_gain: 0.0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
