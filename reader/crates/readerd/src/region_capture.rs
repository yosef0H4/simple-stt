//! Private, bounded screenshot capture for the Linux region reader.
use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LIMIT: u64 = 12 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(180);
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn capture_region(destination: &Path, current: impl Fn() -> bool) -> Result<()> {
    capture_with_executable(destination, &current, Path::new("/usr/bin/lumine-capture"))
}

fn capture_with_executable(
    destination: &Path,
    current: &impl Fn() -> bool,
    executable: &Path,
) -> Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .context("creating private screenshot")?;
    let result = capture(destination, &mut output, current, executable);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn capture(
    destination: &Path,
    output: &mut File,
    current: &impl Fn() -> bool,
    executable: &Path,
) -> Result<()> {
    if !current() {
        bail!("screen-region capture canceled");
    }
    if executable.is_file() {
        capture_lumine(executable, output, current)?;
    } else {
        let source = portal_capture(current)?;
        let _source_cleanup = crate::privacy::TemporaryFile(source.clone());
        let input = File::open(source).context("opening portal screenshot")?;
        let size = std::io::copy(&mut input.take(LIMIT + 1), output)?;
        if size > LIMIT {
            bail!("screenshot exceeds size limit");
        }
    }
    output.flush()?;
    let metadata = fs::metadata(destination)?;
    if metadata.len() < PNG.len() as u64 || metadata.len() > LIMIT {
        bail!("invalid screenshot size");
    }
    let mut signature = [0; 8];
    File::open(destination)?.read_exact(&mut signature)?;
    if signature != PNG {
        bail!("screenshot is not a PNG");
    }
    if !current() {
        bail!("screen-region capture canceled");
    }
    Ok(())
}

fn capture_lumine(executable: &Path, output: &mut File, current: &impl Fn() -> bool) -> Result<()> {
    let mut child = Command::new(executable)
        .args(["-r", "-t", "o", "--nonotify"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting LumineCapture")?;
    let result = (|| -> Result<()> {
        let mut stdout = child.stdout.take().context("opening screenshot output")?;
        let fd = stdout.as_raw_fd();
        // Nonblocking reads let cancellation and timeout interrupt a stalled selector.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let started = Instant::now();
        let mut size = 0_u64;
        let mut eof = false;
        let mut status = None;
        let mut buffer = [0_u8; 16384];
        loop {
            if !current() {
                bail!("screen-region capture canceled");
            }
            if started.elapsed() >= TIMEOUT {
                bail!("screen-region capture timed out");
            }
            if !eof {
                match stdout.read(&mut buffer) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        size += n as u64;
                        if size > LIMIT {
                            bail!("screenshot exceeds size limit");
                        }
                        output.write_all(&buffer[..n])?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if eof && status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !status.expect("checked above").success() {
            bail!("screen-region capture canceled or failed");
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn portal_capture(current: &impl Fn() -> bool) -> Result<PathBuf> {
    use ashpd::desktop::screenshot::{AvailableTargets, Screenshot};
    use futures_lite::future;
    let started = Instant::now();
    let uri = future::block_on(async {
        let request = async {
            let response = Screenshot::request()
                .interactive(true)
                .modal(true)
                .target(AvailableTargets::Area)
                .send()
                .await?;
            Ok::<_, ashpd::Error>(response.response()?.uri().as_str().to_owned())
        };
        let cancel = future::poll_fn(|cx| {
            if !current() {
                return std::task::Poll::Ready(Err(anyhow::anyhow!(
                    "screen-region capture canceled"
                )));
            }
            if started.elapsed() >= TIMEOUT {
                return std::task::Poll::Ready(Err(anyhow::anyhow!(
                    "screen-region capture timed out"
                )));
            }
            std::thread::sleep(Duration::from_millis(50));
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        });
        future::race(async { request.await.map_err(anyhow::Error::from) }, cancel).await
    })
    .context("requesting interactive screenshot portal")?;
    local_file_uri(&uri)
}

fn local_file_uri(uri: &str) -> Result<PathBuf> {
    let path = uri
        .strip_prefix("file://")
        .context("portal returned a non-local screenshot URI")?;
    let path = if path.starts_with('/') {
        path
    } else {
        path.strip_prefix("localhost/")
            .map(|p| &p[0..])
            .context("portal returned a remote screenshot URI")?
    };
    let mut bytes = Vec::with_capacity(path.len());
    let input = path.as_bytes();
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 >= input.len() {
                bail!("invalid screenshot URI escape");
            }
            let hex = std::str::from_utf8(&input[i + 1..i + 3])?;
            bytes.push(u8::from_str_radix(hex, 16).context("invalid screenshot URI escape")?);
            i += 3;
        } else {
            bytes.push(input[i]);
            i += 1;
        }
    }
    if !bytes.starts_with(b"/") {
        bytes.insert(0, b'/');
    }
    if bytes.contains(&0) {
        bail!("invalid screenshot URI");
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("reader-region-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn fake(&self, body: &str) -> PathBuf {
            let path = self.0.join("lumine-capture");
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}/args'\necho $$ > '{}/pid'\n{}\n",
                self.0.display(),
                self.0.display(),
                body
            );
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
        fn assert_args(&self) {
            assert_eq!(
                fs::read_to_string(self.0.join("args")).unwrap(),
                "-r\n-t\no\n--nonotify\n"
            );
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn private_png_capture() {
        let dir = TestDir::new();
        let fixture = dir.0.join("fixture.png");
        fs::write(&fixture, b"\x89PNG\r\n\x1a\nfixture").unwrap();
        let executable = dir.fake(&format!("exec cat '{}'", fixture.display()));
        let destination = dir.0.join("output.png");
        capture_with_executable(&destination, &|| true, &executable).unwrap();
        dir.assert_args();
        assert_eq!(fs::read(&destination).unwrap(), fs::read(&fixture).unwrap());
        assert_eq!(
            fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn oversize_capture_removes_output() {
        let dir = TestDir::new();
        let executable = dir.fake("exec dd if=/dev/zero bs=1048576 count=13 2>/dev/null");
        let destination = dir.0.join("output.png");
        let error = capture_with_executable(&destination, &|| true, &executable).unwrap_err();
        assert!(
            error.to_string().contains("size"),
            "unexpected capture failure: {error:#}"
        );
        dir.assert_args();
        assert!(!destination.exists());
    }

    #[test]
    fn cancel_reaps_owned_child_and_removes_output() {
        let dir = TestDir::new();
        let executable = dir.fake("exec sleep 30");
        let destination = dir.0.join("output.png");
        let start = Instant::now();
        assert!(capture_with_executable(
            &destination,
            &|| start.elapsed() < Duration::from_secs(1),
            &executable
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(10));
        dir.assert_args();
        assert!(!destination.exists());
        let pid: libc::pid_t = fs::read_to_string(dir.0.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
