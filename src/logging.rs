use crate::config::LogLevel;
use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tracing_subscriber::fmt::MakeWriter;

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

struct BoundedLogFile {
    file: File,
    bytes: u64,
}

impl BoundedLogFile {
    fn write_all(&mut self, body: &[u8]) -> std::io::Result<()> {
        if self.bytes.saturating_add(body.len() as u64) > MAX_LOG_BYTES {
            self.file.set_len(0)?;
            self.file.seek(SeekFrom::Start(0))?;
            self.bytes = 0;
        }
        self.file.write_all(body)?;
        self.bytes = self.bytes.saturating_add(body.len() as u64);
        Ok(())
    }
}

#[derive(Clone)]
struct LogWriter {
    file: Arc<Mutex<BoundedLogFile>>,
    prefix: Arc<Vec<u8>>,
}
struct LogGuard {
    file: Arc<Mutex<BoundedLogFile>>,
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
        self.file.lock().unwrap().file.flush()
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
    let stale = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= MAX_LOG_AGE);
    let oversized = fs::metadata(path)
        .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
        .unwrap_or(false);
    let file = open_log_file(path, stale || oversized)
        .with_context(|| format!("opening {}", path.display()))?;
    let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let prefix = format!("component={component} pid={} ", std::process::id()).into_bytes();
    let writer = LogWriter {
        file: Arc::new(Mutex::new(BoundedLogFile { file, bytes })),
        prefix: Arc::new(prefix),
    };
    let effective_level = if cfg!(debug_assertions) {
        level
    } else {
        &LogLevel::Minimal
    };
    let filter = if cfg!(debug_assertions) {
        std::env::var("RUST_LOG").unwrap_or_else(|_| effective_level.tracing_filter().to_owned())
    } else {
        effective_level.tracing_filter().to_owned()
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(writer)
        .try_init()
        .ok();
    tracing::info!(log = %path.display(), "component logging initialized");
    Ok(())
}

fn open_log_file(path: &Path, reset: bool) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true);
    if reset {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    options.open(path)
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

    #[test]
    fn bounded_log_discards_old_content_at_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bounded.log");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut log = BoundedLogFile { file, bytes: 0 };
        log.write_all(&vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        log.write_all(b"newest").unwrap();
        log.file.flush().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"newest");
    }

    #[test]
    fn stale_log_is_reopened_for_truncation_without_append_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stale.log");
        fs::write(&path, b"old content").unwrap();
        let mut file = open_log_file(&path, true).unwrap();
        file.write_all(b"fresh").unwrap();
        file.flush().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"fresh");
    }
}
