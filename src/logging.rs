use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

use crate::config::{AppConfig, LogLevel};

#[derive(Clone)]
struct LogWriter {
    file: Arc<Mutex<File>>,
}

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = LogGuard;

    fn make_writer(&'a self) -> Self::Writer {
        LogGuard {
            file: Arc::clone(&self.file),
        }
    }
}

struct LogGuard {
    file: Arc<Mutex<File>>,
}

impl std::io::Write for LogGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write(buf);
        self.file.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.file.lock().unwrap().flush()
    }
}

pub fn init(config: Option<&AppConfig>) -> Result<()> {
    init_with_append(config, false)
}

pub fn init_append(config: Option<&AppConfig>) -> Result<()> {
    init_with_append(config, true)
}

fn init_with_append(config: Option<&AppConfig>, append: bool) -> Result<()> {
    let level = config
        .map(|config| config.log_level.clone())
        .unwrap_or(LogLevel::Normal);
    let path = AppConfig::log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = if append {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?
    } else {
        File::create(&path).with_context(|| format!("creating {}", path.display()))?
    };
    let writer = LogWriter {
        file: Arc::new(Mutex::new(file)),
    };
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| level.tracing_filter().to_owned());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(writer)
        .init();
    tracing::info!(path = %path.display(), append, "logging initialized");
    Ok(())
}
