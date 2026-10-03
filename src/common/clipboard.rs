//! Bounded clipboard publication. A launched process is not a ready selection.
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);
const READ_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_READ: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
pub enum ClipboardKind {
    Wayland,
    Xclip,
    Xsel,
}

pub struct ClipboardTools {
    pub kind: ClipboardKind,
    pub publisher: PathBuf,
    pub reader: PathBuf,
}

/// Owns the exact child while publication is pending. Failed/cancelled attempts
/// are killed and reaped; a successful owner can outlive the one-shot shell.
pub struct PublishedClipboard {
    child: Option<Child>,
    tools: ClipboardTools,
    primary: bool,
    text: String,
}

impl Drop for PublishedClipboard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl PublishedClipboard {
    pub fn verify(&self) -> Result<bool> {
        let mut command = self.tools.read_command(self.primary);
        let output = output_bounded(&mut command, READ_TIMEOUT)?;
        Ok(output.status.success() && output.stdout == self.text.as_bytes())
    }

    /// Leave the immutable payload available for a target which reads later.
    /// The clipboard tool exits when another owner replaces the selection.
    pub fn detach(mut self) {
        drop(self.child.take());
    }
}

impl ClipboardTools {
    fn publish_command(&self, primary: bool) -> Command {
        let mut command = Command::new(&self.publisher);
        match self.kind {
            ClipboardKind::Wayland => {
                command.args(["--foreground", "--type", "text/plain;charset=utf-8"]);
                if primary {
                    command.arg("--primary");
                }
            }
            ClipboardKind::Xclip => {
                command.args(["-quiet", "-selection", selection(primary)]);
            }
            ClipboardKind::Xsel => {
                command.args(["--nodetach", "--input", selection_flag(primary)]);
            }
        }
        command
    }

    fn read_command(&self, primary: bool) -> Command {
        let mut command = Command::new(&self.reader);
        match self.kind {
            ClipboardKind::Wayland => {
                command.args(["--no-newline", "--type", "text"]);
                if primary {
                    command.arg("--primary");
                }
            }
            ClipboardKind::Xclip => {
                command.args(["-out", "-selection", selection(primary)]);
            }
            ClipboardKind::Xsel => {
                command.args(["--output", selection_flag(primary)]);
            }
        }
        command
    }

    pub fn publish_private(
        self,
        text: &str,
        primary: bool,
        is_current: impl Fn() -> bool,
        timeout: Duration,
        helper: &std::path::Path,
    ) -> Result<PublishedClipboard> {
        let mut command = match self.kind {
            ClipboardKind::Wayland => {
                let mut command = self.publish_command(primary);
                command.arg("--sensitive");
                command
            }
            ClipboardKind::Xclip | ClipboardKind::Xsel => {
                let mut command = Command::new(helper);
                command.arg("clipboard-owner");
                if primary {
                    command.arg("--primary");
                }
                command
            }
        };
        self.publish_with_command(text, primary, is_current, timeout, &mut command)
    }

    pub fn publish(
        self,
        text: &str,
        primary: bool,
        is_current: impl Fn() -> bool,
        timeout: Duration,
    ) -> Result<PublishedClipboard> {
        let mut command = self.publish_command(primary);
        self.publish_with_command(text, primary, is_current, timeout, &mut command)
    }

    fn publish_with_command(
        self,
        text: &str,
        primary: bool,
        is_current: impl Fn() -> bool,
        timeout: Duration,
        command: &mut Command,
    ) -> Result<PublishedClipboard> {
        if !is_current() {
            bail!("dictation superseded before clipboard publication");
        }
        let deadline = Instant::now() + timeout;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting clipboard publisher")?;
        let mut stdin = child.stdin.take().context("opening clipboard input")?;
        let payload = text.as_bytes().to_vec();
        let (sender, receiver) = mpsc::sync_channel(1);
        let writer = std::thread::spawn(move || {
            let result = stdin.write_all(&payload);
            drop(stdin); // Publishers need EOF before exposing the selection.
            let _ = sender.send(result);
        });
        let mut owner = PublishedClipboard {
            child: Some(child),
            tools: self,
            primary,
            text: text.to_owned(),
        };
        let input_result = (|| -> Result<()> {
            loop {
                if !is_current() {
                    bail!("dictation superseded during clipboard publication");
                }
                match receiver.recv_timeout(POLL) {
                    Ok(result) => return result.context("writing clipboard input"),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        bail!("clipboard input writer exited")
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= deadline {
                    bail!("clipboard input timed out");
                }
            }
        })();
        if input_result.is_err() {
            drop(owner); // Close the child's pipe before joining the writer.
            let _ = writer.join();
            input_result?;
            unreachable!();
        }
        let _ = writer.join();
        loop {
            if !is_current() {
                bail!("dictation superseded during clipboard publication");
            }
            if let Some(status) = owner.child.as_mut().unwrap().try_wait()? {
                if !status.success() {
                    bail!("clipboard publisher failed");
                }
            }
            if owner.verify().unwrap_or(false) && is_current() {
                return Ok(owner);
            }
            if Instant::now() >= deadline {
                bail!("clipboard text did not become available; paste was not sent");
            }
            std::thread::sleep(POLL);
        }
    }
}

fn selection(primary: bool) -> &'static str {
    if primary {
        "primary"
    } else {
        "clipboard"
    }
}

fn selection_flag(primary: bool) -> &'static str {
    if primary {
        "--primary"
    } else {
        "--clipboard"
    }
}

/// Drain stdout concurrently so large payloads cannot deadlock a child. All
/// callers use foreground, non-forking readers/helpers with closed stderr.
pub fn output_bounded(command: &mut Command, timeout: Duration) -> Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting clipboard or input helper")?;
    let stdout = child.stdout.take().context("opening helper output")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(MAX_READ + 1).read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break match result {
                    Err(error) => Err(anyhow::Error::from(error)),
                    _ => Err(anyhow::anyhow!("clipboard or input helper timed out")),
                };
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("helper output reader panicked"))??;
    if bytes.len() as u64 > MAX_READ {
        bail!("clipboard or input helper output exceeded limit");
    }
    Ok(Output {
        status: status?,
        stdout: bytes,
        stderr: Vec::new(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn tools(dir: &std::path::Path, delay: f64, fail: bool) -> ClipboardTools {
        let clipboard = dir.join("payload");
        fs::write(&clipboard, "OLD_TEXT").unwrap();
        let publisher = dir.join("copy");
        let reader = dir.join("read");
        let code = format!(
            "#!/usr/bin/python3\nimport pathlib,sys,time\np=pathlib.Path({:?})\ntext=sys.stdin.buffer.read()\ntime.sleep({delay})\nif {fail}:sys.exit(7)\np.write_bytes(text)\nwhile True:time.sleep(1)\n",
            clipboard.to_str().unwrap(), fail = if fail { "True" } else { "False" }
        );
        fs::write(&publisher, code).unwrap();
        fs::write(&reader, format!("#!/usr/bin/python3\nimport pathlib,sys\nsys.stdout.buffer.write(pathlib.Path({:?}).read_bytes())\n", clipboard.to_str().unwrap())).unwrap();
        for path in [&publisher, &reader] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        ClipboardTools {
            kind: ClipboardKind::Wayland,
            publisher,
            reader,
        }
    }

    #[test]
    fn delayed_publication_waits_for_exact_unicode_and_eof() {
        let dir = tempfile::tempdir().unwrap();
        let start = Instant::now();
        let text = "مرحبا\nEnglish 🙂\n";
        let owner = tools(dir.path(), 0.6, false)
            .publish(text, false, || true, Duration::from_secs(2))
            .unwrap();
        assert!(start.elapsed() >= Duration::from_millis(600));
        assert!(owner.verify().unwrap());
        // A slow target must still obtain the transcript after the former
        // 250 ms restore window. A readiness read must not consume the offer.
        std::thread::sleep(Duration::from_millis(650));
        assert!(owner.verify().unwrap());
        assert_eq!(
            fs::read_to_string(dir.path().join("payload")).unwrap(),
            text
        );
    }

    #[test]
    fn failed_publisher_is_not_reported_ready() {
        let dir = tempfile::tempdir().unwrap();
        assert!(tools(dir.path(), 0.0, true)
            .publish("NEW_TEXT", false, || true, Duration::from_secs(1))
            .is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("payload")).unwrap(),
            "OLD_TEXT"
        );
    }

    #[test]
    fn cancelled_publication_is_killed_before_it_can_arrive_late() {
        let dir = tempfile::tempdir().unwrap();
        let start = Instant::now();
        assert!(tools(dir.path(), 0.6, false)
            .publish(
                "OBSOLETE",
                false,
                || start.elapsed() < Duration::from_millis(100),
                Duration::from_secs(2)
            )
            .is_err());
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            fs::read_to_string(dir.path().join("payload")).unwrap(),
            "OLD_TEXT"
        );
    }

    #[test]
    fn publication_timeout_reaps_owner_and_preserves_user_copy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(tools(dir.path(), 0.6, false)
            .publish("LATE", false, || true, Duration::from_millis(100))
            .is_err());
        fs::write(dir.path().join("payload"), "NEW_USER_COPY").unwrap();
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(
            fs::read_to_string(dir.path().join("payload")).unwrap(),
            "NEW_USER_COPY"
        );
    }

    #[test]
    fn user_clipboard_change_prevents_verification_and_is_not_restored() {
        let dir = tempfile::tempdir().unwrap();
        let owner = tools(dir.path(), 0.0, false)
            .publish("NEW_TEXT", false, || true, Duration::from_secs(1))
            .unwrap();
        fs::write(dir.path().join("payload"), "NEW_USER_COPY").unwrap();
        assert!(!owner.verify().unwrap());
        drop(owner);
        assert_eq!(
            fs::read_to_string(dir.path().join("payload")).unwrap(),
            "NEW_USER_COPY"
        );
    }

    #[test]
    fn stalled_reader_has_bounded_lifetime() {
        let mut command = Command::new("/usr/bin/python3");
        command.args(["-c", "import time;time.sleep(10)"]);
        let start = Instant::now();
        assert!(output_bounded(&mut command, Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn blocked_input_writer_is_killed_and_joined_on_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let tools = tools(dir.path(), 0.0, false);
        fs::write(
            &tools.publisher,
            "#!/usr/bin/python3\nimport time\ntime.sleep(10)\n",
        )
        .unwrap();
        let start = Instant::now();
        assert!(tools
            .publish(
                &"x".repeat(1024 * 1024),
                false,
                || true,
                Duration::from_millis(100)
            )
            .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn large_transcript_uses_stdin_instead_of_argument_limits() {
        let dir = tempfile::tempdir().unwrap();
        let text = "مرحبا 🙂\n".repeat(30_000);
        let owner = tools(dir.path(), 0.0, false)
            .publish(&text, false, || true, Duration::from_secs(2))
            .unwrap();
        assert!(owner.verify().unwrap());
    }
}
