use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub idle_timeout_secs: u64,
    pub typing_interval_ms: u64,
    pub typing_chunk_chars: usize,
    pub ring_buffer_secs: usize,
    pub audio_gain: f32,
    pub audio_device_contains: String,
    pub lookahead_ms: u32,
    pub python_executable: String,
    pub worker_dir: String,
    pub worker_backend: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 180,
            typing_interval_ms: 20,
            typing_chunk_chars: 3,
            ring_buffer_secs: 8,
            audio_gain: 1.0,
            audio_device_contains: String::new(),
            lookahead_ms: 80,
            python_executable: r"worker\.venv\Scripts\python.exe".to_owned(),
            worker_dir: "worker".to_owned(),
            worker_backend: "nemotron".to_owned(),
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
            self.ring_buffer_secs > 0,
            "ring_buffer_secs must be positive"
        );
        anyhow::ensure!(
            self.audio_gain > 0.0 && self.audio_gain <= 10.0,
            "audio_gain must be in (0, 10]"
        );
        anyhow::ensure!(
            [0, 80, 480, 1040].contains(&self.lookahead_ms),
            "lookahead_ms must be 0, 80, 480, or 1040"
        );
        anyhow::ensure!(
            matches!(
                self.worker_backend.as_str(),
                "nemotron" | "echo" | "parakeet-record"
            ),
            "worker_backend must be nemotron, echo, or parakeet-record"
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
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust crate must live below repo root")
        .to_path_buf()
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
    fn invalid_gain_is_rejected() {
        let config = AppConfig {
            audio_gain: 0.0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
