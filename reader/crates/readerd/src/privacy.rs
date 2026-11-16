//! Private processing files and content-free default diagnostics.
use anyhow::{Context, Result};
use std::{
    fmt,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{
    fmt::{
        format::{FormatEvent, FormatFields, Writer},
        FmtContext,
    },
    registry::LookupSpan,
};

static CONTENT: AtomicBool = AtomicBool::new(false);
pub fn set_content_logging(enabled: bool) {
    CONTENT.store(enabled, Ordering::Relaxed);
}
pub fn content_logging() -> bool {
    CONTENT.load(Ordering::Relaxed)
}

pub struct LogFormat {
    content_logging: fn() -> bool,
}

impl LogFormat {
    pub fn with_policy(content_logging: fn() -> bool) -> Self {
        Self { content_logging }
    }
}

impl Default for LogFormat {
    fn default() -> Self {
        Self::with_policy(content_logging)
    }
}

impl<S, N> FormatEvent<S, N> for LogFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        if (self.content_logging)() {
            return tracing_subscriber::fmt::format().format_event(context, writer, event);
        }
        let metadata = event.metadata();
        write!(
            writer,
            "{} {}:{}",
            metadata.level(),
            metadata.target(),
            metadata.line().unwrap_or(0)
        )?;
        let mut fields = MetadataFields {
            writer: &mut writer,
            result: Ok(()),
        };
        event.record(&mut fields);
        fields.result?;
        writeln!(writer)
    }
}
struct MetadataFields<'a, 'b> {
    writer: &'a mut Writer<'b>,
    result: fmt::Result,
}
impl Visit for MetadataFields<'_, '_> {
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.result = write!(self.writer, " {}={value}", field.name());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.result = write!(self.writer, " {}={value}", field.name());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.result = write!(self.writer, " {}={value}", field.name());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if matches!(field.name(), "feature" | "phase" | "stage") {
            self.result = write!(self.writer, " {}={value:?}", field.name());
        } else {
            self.result = write!(
                self.writer,
                " {}_chars={}",
                field.name(),
                value.chars().count()
            );
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let count = format!("{value:?}").chars().count();
        self.result = write!(self.writer, " {}_chars={count}", field.name());
    }
}

pub fn text_metadata(feature: &str, text: &str) {
    if content_logging() {
        tracing::info!(
            feature,
            chars = text.chars().count(),
            text,
            "content logging explicitly enabled"
        );
    } else {
        tracing::info!(feature, chars = text.chars().count(), "text processed");
    }
}

/// Own only an internally generated file, never an imported/user-selected file.
pub struct TemporaryFile(pub PathBuf);
impl TemporaryFile {
    pub fn create(path: PathBuf, data: &[u8]) -> Result<Self> {
        let owned = Self(path);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&owned.0)?.write_all(data)?;
        Ok(owned)
    }
}
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// One controller owns a profile's processing directories, including on a VPS.
pub struct ProcessingLease {
    _file: File,
}
impl ProcessingLease {
    pub fn acquire(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let file = options.open(directory.join("processing.lock"))?;
        file.try_lock()
            .context("another controller owns this profile")?;
        for name in ["capture", "requests", "cli"] {
            let path = directory.join(name);
            if !path.is_dir() {
                continue;
            }
            for item in std::fs::read_dir(path)? {
                let item = item?;
                let name = item.file_name().to_string_lossy().into_owned();
                if item.file_type()?.is_file()
                    && [
                        "server-",
                        "transcription-",
                        "read-along-",
                        "clipboard-",
                        "cli-",
                        "region-",
                        "capture-",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                {
                    std::fs::remove_file(item.path())?;
                }
            }
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn redact_content() -> bool {
        false
    }

    fn allow_content() -> bool {
        true
    }

    fn formatted_event(policy: fn() -> bool) -> String {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(SharedWriter(Arc::clone(&output)))
            .event_format(LogFormat::with_policy(policy))
            .finish();
        let sentinel = "SSTT_PRIVATE_SENTINEL_314159";
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(
                text = %sentinel,
                error = %sentinel,
                message = %sentinel,
                chars = 23,
                "failure: {sentinel}"
            );
        });
        let bytes = output.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn default_log_format_keeps_counts_and_redacts_text_error_and_message() {
        let output = formatted_event(redact_content);
        assert!(!output.contains("SSTT_PRIVATE_SENTINEL_314159"), "{output}");
        assert!(output.contains("chars=23"), "{output}");
        assert!(output.contains("text_chars="), "{output}");
        assert!(output.contains("error_chars="), "{output}");
        assert!(output.contains("message_chars="), "{output}");
    }

    #[test]
    fn content_log_format_includes_content_only_when_policy_allows_it() {
        let output = formatted_event(allow_content);
        assert!(output.contains("SSTT_PRIVATE_SENTINEL_314159"), "{output}");
        assert!(output.contains("failure:"), "{output}");
    }

    #[test]
    fn temporary_content_is_removed_on_error_unwind() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("server-test.img");
        let result: Result<()> = (|| {
            let _file = TemporaryFile::create(path.clone(), b"private image").unwrap();
            anyhow::bail!("cancelled")
        })();
        assert!(result.is_err());
        assert!(!path.exists());
    }
    #[test]
    fn restart_removes_owned_content_and_preserves_exports() {
        let folder = tempfile::tempdir().unwrap();
        let requests = folder.path().join("requests");
        std::fs::create_dir(&requests).unwrap();
        std::fs::write(requests.join("transcription-crashed.wav"), b"audio").unwrap();
        std::fs::write(requests.join("my-export.wav"), b"explicit export").unwrap();
        let _lease = ProcessingLease::acquire(folder.path()).unwrap();
        assert!(!requests.join("transcription-crashed.wav").exists());
        assert!(requests.join("my-export.wav").exists());
        assert!(ProcessingLease::acquire(folder.path()).is_err());
    }
}
