use anyhow::{bail, Context, Result};
use crossbeam_channel::Sender;
use serde_json::{json, Value};
use std::io;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::AppConfig;
use crate::protocol::{parse_json, read_frame, write_json, write_pcm16};

#[cfg(windows)]
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Loading,
    Ready,
    Partial { session_id: u64, text: String },
    Commit { session_id: u64, text: String },
    Status(String),
    Error(String),
    Disconnected,
}

pub struct WorkerHandle {
    writer: Arc<Mutex<TcpStream>>,
    child: Child,
}

impl WorkerHandle {
    pub fn spawn(config: &AppConfig, event_tx: Sender<WorkerEvent>) -> Result<Self> {
        let listener =
            TcpListener::bind("127.0.0.1:0").context("binding worker callback socket")?;
        let address = listener.local_addr()?;
        let token = random_token()?;
        let python = config.resolve_from_repo(&config.python_executable);
        let worker_dir = config.resolve_from_repo(&config.worker_dir);
        tracing::debug!(
            %address,
            python = %python.display(),
            worker_dir = %worker_dir.display(),
            backend = %config.worker_backend,
            lookahead_ms = config.lookahead_ms,
            "prepared worker callback listener"
        );
        anyhow::ensure!(
            python.exists(),
            "Python worker executable is missing: {}. Run scripts/setup-worker.ps1",
            python.display()
        );
        anyhow::ensure!(
            worker_dir.exists(),
            "Worker directory is missing: {}",
            worker_dir.display()
        );

        let mut command = Command::new(&python);
        command
            .current_dir(&worker_dir)
            .arg("-m")
            .arg("uvox_worker")
            .arg("serve")
            .arg("--connect")
            .arg(address.to_string())
            .arg("--token")
            .arg(&token)
            .arg("--lookahead-ms")
            .arg(config.lookahead_ms.to_string())
            .arg("--backend")
            .arg(&config.worker_backend)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        #[cfg(windows)]
        if std::env::var_os("UVOX_SHOW_WORKER_CONSOLE").is_none() {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        tracing::info!("spawning Python worker process");
        let child = command
            .spawn()
            .with_context(|| format!("starting {}", python.display()))?;
        listener.set_nonblocking(false)?;
        tracing::debug!("waiting for Python worker callback connection");
        let (stream, _) = listener
            .accept()
            .context("accepting Python worker callback")?;
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;

        let hello = parse_json(&read_frame(&mut reader).context("reading worker hello")?)?;
        tracing::debug!(%hello, "received worker hello");
        let received_token = hello
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if received_token != token {
            bail!("worker authentication token mismatch");
        }
        if hello.get("protocol").and_then(Value::as_u64) != Some(1) {
            bail!("unsupported worker protocol version: {hello}");
        }

        thread::spawn(move || loop {
            match read_frame(&mut reader).and_then(|frame| parse_json(&frame)) {
                Ok(message) => {
                    tracing::debug!(%message, "received worker JSON event");
                    if let Some(event) = map_event(message) {
                        let _ = event_tx.send(event);
                    }
                }
                Err(error) if error.downcast_ref::<io::Error>().is_some() => {
                    let _ = event_tx.send(WorkerEvent::Disconnected);
                    return;
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::Error(error.to_string()));
                    return;
                }
            }
        });

        Ok(Self {
            writer: Arc::new(Mutex::new(stream)),
            child,
        })
    }

    pub fn start_session(&self, session_id: u64) -> Result<()> {
        tracing::debug!(session_id, "sending worker start");
        self.send_json(&json!({"type":"start","session_id":session_id}))
    }

    pub fn start_recording(&self, session_id: u64) -> Result<()> {
        tracing::debug!(session_id, "sending worker recording start");
        self.send_json(&json!({"type":"transcribe_recording","session_id":session_id}))
    }

    pub fn finish_recording(&self, session_id: u64) -> Result<()> {
        tracing::debug!(session_id, "sending worker recording finish");
        self.send_json(&json!({"type":"finish_recording","session_id":session_id}))
    }

    pub fn cancel_session(&self, session_id: u64) -> Result<()> {
        tracing::debug!(session_id, "sending worker cancel");
        self.send_json(&json!({"type":"cancel","session_id":session_id}))
    }

    pub fn send_audio(&self, session_id: u64, samples: &[i16]) -> Result<()> {
        tracing::trace!(
            session_id,
            samples = samples.len(),
            "sending PCM frame to worker"
        );
        write_pcm16(&mut *self.writer.lock().unwrap(), session_id, samples)
    }

    fn send_json(&self, message: &Value) -> Result<()> {
        write_json(&mut *self.writer.lock().unwrap(), message)
    }

    pub fn shutdown(mut self) {
        let _ = self.send_json(&json!({"type":"shutdown"}));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn map_event(message: Value) -> Option<WorkerEvent> {
    let kind = message.get("type")?.as_str()?;
    match kind {
        "status" => {
            let state = message.get("state")?.as_str()?.to_owned();
            Some(match state.as_str() {
                "loading_model" => WorkerEvent::Loading,
                "ready" => WorkerEvent::Ready,
                _ => WorkerEvent::Status(state),
            })
        }
        "partial" => Some(WorkerEvent::Partial {
            session_id: message.get("session_id")?.as_u64()?,
            text: message.get("text")?.as_str()?.to_owned(),
        }),
        "commit" => Some(WorkerEvent::Commit {
            session_id: message.get("session_id")?.as_u64()?,
            text: message.get("text")?.as_str()?.to_owned(),
        }),
        "error" => Some(WorkerEvent::Error(
            message.get("message")?.as_str()?.to_owned(),
        )),
        other => Some(WorkerEvent::Status(other.to_owned())),
    }
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> Result<()> {
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    anyhow::ensure!(
        status >= 0,
        "BCryptGenRandom failed with status {status:#x}"
    );
    Ok(())
}

#[cfg(not(windows))]
fn fill_random(_bytes: &mut [u8]) -> Result<()> {
    bail!("worker token generation is implemented for Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_commit_event() {
        match map_event(json!({"type":"commit","session_id":7,"text":"hello "})).unwrap() {
            WorkerEvent::Commit { session_id, text } => {
                assert_eq!(session_id, 7);
                assert_eq!(text, "hello ");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
