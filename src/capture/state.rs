use crate::common::shell_protocol::SHELL_PROTOCOL_VERSION;
use crate::config::replace_file_atomic;
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
    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }
    pub fn save_atomic(&self, path: &Path) -> Result<()> {
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
        replace_file_atomic(&temp, path).with_context(|| format!("publishing {}", path.display()))
    }
}
