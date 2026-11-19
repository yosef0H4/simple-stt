use crate::common::shell_protocol::SHELL_PROTOCOL_VERSION;
use crate::config::{replace_file_atomic, unique_atomic_temp_path};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub protocol: u32,
    pub pid: u32,
    pub address: String,
    pub started_unix_ms: u128,
}

impl ServiceState {
    pub fn new(address: String) -> Self {
        Self {
            protocol: SHELL_PROTOCOL_VERSION,
            pid: std::process::id(),
            address,
            started_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        }
    }
    pub fn authentication_token(&self) -> Result<String> {
        #[cfg(windows)]
        return reader_core::secrets::capture_token(&crate::config::app_instance_id(), self.pid)?
            .context("capture authentication unavailable; restart the installed Simple STT app");
        #[cfg(not(windows))]
        fs::read_to_string(crate::config::AppConfig::local_data_dir().join("linux-token"))
            .context("capture service authentication token unavailable; start the installed Simple STT desktop app")
    }
    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }
    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let temp = unique_atomic_temp_path(path);
        {
            let mut file =
                fs::File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
            file.write_all((serde_json::to_string_pretty(self)? + "\n").as_bytes())?;
            file.flush()?;
            file.sync_all()?;
        }
        replace_file_atomic(&temp, path).with_context(|| format!("publishing {}", path.display()))
    }
}

#[cfg(windows)]
pub struct PublishedCaptureToken {
    instance: String,
    pid: u32,
}
#[cfg(windows)]
impl PublishedCaptureToken {
    pub fn new(token: &str) -> Result<Self> {
        let instance = crate::config::app_instance_id();
        let pid = std::process::id();
        reader_core::secrets::set_capture_token(&instance, pid, token)?;
        Ok(Self { instance, pid })
    }
}
#[cfg(windows)]
impl Drop for PublishedCaptureToken {
    fn drop(&mut self) {
        let _ = reader_core::secrets::delete_capture_token(&self.instance, self.pid);
    }
}
