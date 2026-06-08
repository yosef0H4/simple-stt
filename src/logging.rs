use crate::config::LogLevel;
use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
struct LogWriter {
    file: Arc<Mutex<File>>,
    prefix: Arc<Vec<u8>>,
}
struct LogGuard {
    file: Arc<Mutex<File>>,
    prefix: Arc<Vec<u8>>,
    at_line_start: bool,
}

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = LogGuard;
    fn make_writer(&'a self) -> Self::Writer {
        LogGuard {
            file: Arc::clone(&self.file),
            prefix: Arc::clone(&self.prefix),
            at_line_start: true,
        }
    }
}
impl std::io::Write for LogGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let decorated = prefix_lines(&self.prefix, buf, &mut self.at_line_start);
        let _ = std::io::stderr().write_all(&decorated);
        self.file.lock().unwrap().write_all(&decorated)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.file.lock().unwrap().flush()
    }
}

fn prefix_lines(prefix: &[u8], buf: &[u8], at_line_start: &mut bool) -> Vec<u8> {
    let mut output = Vec::with_capacity(buf.len() + prefix.len());
    for byte in buf {
        if *at_line_start {
            output.extend_from_slice(prefix);
            *at_line_start = false;
        }
        output.push(*byte);
        if *byte == b'\n' {
            *at_line_start = true;
        }
    }
    output
}

pub fn init_component(component: &str, path: &Path, level: &LogLevel) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let prefix = format!("component={component} pid={} ", std::process::id()).into_bytes();
    let writer = LogWriter {
        file: Arc::new(Mutex::new(file)),
        prefix: Arc::new(prefix),
    };
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| level.tracing_filter().to_owned());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(writer)
        .try_init()
        .ok();
    tracing::info!(log = %path.display(), "component logging initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_prefix_survives_split_writes_and_multiline_events() {
        let mut at_line_start = true;
        let first = prefix_lines(
            b"component=capture pid=42 ",
            b"first\nsec",
            &mut at_line_start,
        );
        let second = prefix_lines(b"component=capture pid=42 ", b"ond\n", &mut at_line_start);
        assert_eq!(
            String::from_utf8(first).unwrap(),
            "component=capture pid=42 first\ncomponent=capture pid=42 sec"
        );
        assert_eq!(String::from_utf8(second).unwrap(), "ond\n");
        assert!(at_line_start);
    }
}
