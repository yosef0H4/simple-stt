use anyhow::{Context, Result};
use std::process::Command;

use crate::config::AppConfig;

pub fn show_settings() -> Result<()> {
    let config = AppConfig::load()?;
    let path = AppConfig::config_path();
    config.save_to(&path)?;
    Command::new("notepad.exe")
        .arg(&path)
        .spawn()
        .with_context(|| format!("opening settings file {}", path.display()))?;
    println!("Opened settings file: {}", path.display());
    println!("Save the file and restart `uvox run` to apply changes.");
    Ok(())
}
