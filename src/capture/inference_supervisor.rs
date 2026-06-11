use crate::capture::process::force_terminate_pid;
use crate::config::{InferenceDevice, LogLevel};
use crate::infer::protocol::{read_frame, write_frame, Frame, MessageType};
use anyhow::{Context, Result};
use crossbeam_channel::{bounded, RecvTimeoutError};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

const SHARED_SHUTDOWN_OVERHEAD: Duration = Duration::from_millis(250);
const SHARED_SHUTDOWN_RECOVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub executable: PathBuf,
    pub runtime_dir: PathBuf,
    pub model_path: PathBuf,
    pub log_path: PathBuf,
    pub log_level: LogLevel,
    pub inference_device: InferenceDevice,
    pub idle_timeout: Duration,
    pub shutdown_grace: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Launch,
    Reuse,
    ShutDownIdle,
    ReplaceModel,
}

#[derive(Debug, Default)]
pub struct LifecyclePolicy {
    pub running: bool,
    pub last_used: Option<Instant>,
    pub model_path: Option<PathBuf>,
}
impl LifecyclePolicy {
    pub fn before_request(&mut self, model_path: &Path) -> LifecycleAction {
        let action = if !self.running {
            LifecycleAction::Launch
        } else if self.model_path.as_deref() != Some(model_path) {
            LifecycleAction::ReplaceModel
        } else {
            LifecycleAction::Reuse
        };
        self.running = true;
        self.model_path = Some(model_path.to_path_buf());
        self.last_used = Some(Instant::now());
        action
    }
    pub fn idle_action(&self, now: Instant, timeout: Duration) -> Option<LifecycleAction> {
        self.running
            .then_some(self.last_used)
            .flatten()
            .filter(|last| now.duration_since(*last) >= timeout)
            .map(|_| LifecycleAction::ShutDownIdle)
    }
    pub fn stopped(&mut self) {
        self.running = false;
        self.last_used = None;
    }
}

pub struct WorkerSupervisor {
    config: WorkerConfig,
    worker: Option<WorkerHandle>,
    policy: LifecyclePolicy,
    pid_tracker: Arc<AtomicU32>,
}
struct WorkerHandle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl WorkerSupervisor {
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            worker: None,
            policy: LifecyclePolicy::default(),
            pid_tracker: Arc::new(AtomicU32::new(0)),
        }
    }
    pub fn pid_tracker(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.pid_tracker)
    }
    pub fn worker_pid(&self) -> Option<u32> {
        nonzero_pid(&self.pid_tracker)
    }
    pub fn replace_config(&mut self, next: WorkerConfig) -> Result<()> {
        if self.config.runtime_dir != next.runtime_dir
            || self.config.model_path != next.model_path
            || self.config.log_level != next.log_level
            || self.config.inference_device != next.inference_device
            || self.config.idle_timeout != next.idle_timeout
            || self.config.shutdown_grace != next.shutdown_grace
        {
            tracing::info!(old_model = %self.config.model_path.display(), new_model = %next.model_path.display(), "worker configuration changed; recycling disposable worker");
            self.shutdown_now()?;
        }
        self.config = next;
        Ok(())
    }
    pub fn warm_up(&mut self, mut on_model_loaded: impl FnMut()) -> Result<()> {
        self.ensure_worker()?;
        let worker = self.worker.as_mut().unwrap();
        write_frame(&mut worker.stdin, &Frame::empty(MessageType::WarmUp))
            .context("requesting inference-worker warm-up")?;
        match read_frame(&mut worker.stdout).context("reading model-loaded warm-up progress")? {
            frame if frame.kind == MessageType::ModelLoaded => on_model_loaded(),
            frame if frame.kind == MessageType::Error => anyhow::bail!(frame.body_as_text()?),
            frame => anyhow::bail!("unexpected warm-up progress response: {:?}", frame.kind),
        }
        match read_frame(&mut worker.stdout)
            .context("reading inference-worker warm-up completion")?
        {
            frame if frame.kind == MessageType::WarmUpAck => Ok(()),
            frame if frame.kind == MessageType::Error => anyhow::bail!(frame.body_as_text()?),
            frame => anyhow::bail!("unexpected warm-up completion response: {:?}", frame.kind),
        }
    }
    pub fn transcribe_pcm(&mut self, session_id: u64, samples: &[i16]) -> Result<String> {
        self.ensure_worker()?;
        let result = {
            let worker = self.worker.as_mut().unwrap();
            write_frame(
                &mut worker.stdin,
                &Frame::transcribe_pcm(session_id, 16_000, samples),
            )
            .context("sending PCM to inference worker")?;
            read_frame(&mut worker.stdout).context("reading transcript from inference worker")
        };
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                self.discard_crashed_worker();
                return Err(error);
            }
        };
        self.policy.last_used = Some(Instant::now());
        match frame.kind {
            MessageType::Transcript => frame.body_as_text(),
            MessageType::Error => anyhow::bail!(frame.body_as_text()?),
            other => anyhow::bail!("unexpected inference response: {other:?}"),
        }
    }
    pub fn transcribe_wav(&mut self, session_id: u64, path: &Path) -> Result<String> {
        self.ensure_worker()?;
        let result = {
            let worker = self.worker.as_mut().unwrap();
            write_frame(
                &mut worker.stdin,
                &Frame::text(
                    MessageType::TranscribeWav,
                    session_id,
                    path.to_string_lossy(),
                ),
            )
            .context("sending WAV test to inference worker")?;
            read_frame(&mut worker.stdout).context("reading WAV-test response")
        };
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                self.discard_crashed_worker();
                return Err(error);
            }
        };
        self.policy.last_used = Some(Instant::now());
        match frame.kind {
            MessageType::Transcript => frame.body_as_text(),
            MessageType::Error => anyhow::bail!(frame.body_as_text()?),
            other => anyhow::bail!("unexpected WAV-test response: {other:?}"),
        }
    }
    pub fn shutdown_if_idle(&mut self) -> Result<bool> {
        if self
            .policy
            .idle_action(Instant::now(), self.config.idle_timeout)
            == Some(LifecycleAction::ShutDownIdle)
        {
            tracing::info!(
                timeout_secs = self.config.idle_timeout.as_secs(),
                "idle timeout reached; terminating disposable inference worker"
            );
            self.shutdown_now()?;
            return Ok(true);
        }
        Ok(false)
    }
    pub fn shutdown_now(&mut self) -> Result<()> {
        let Some(mut worker) = self.worker.take() else {
            self.mark_stopped();
            return Ok(());
        };
        let pid = worker.child.id();
        tracing::info!(pid, "requesting graceful inference-worker shutdown");
        let _ = write_frame(&mut worker.stdin, &Frame::empty(MessageType::Shutdown));
        let deadline = Instant::now() + self.config.shutdown_grace;
        loop {
            if worker.child.try_wait()?.is_some() {
                tracing::info!(pid, "inference worker exited");
                self.mark_stopped();
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        tracing::warn!(
            pid,
            "inference worker exceeded graceful-shutdown timeout; forcing termination"
        );
        worker
            .child
            .kill()
            .context("force-terminating inference worker")?;
        let _ = worker.child.wait();
        self.mark_stopped();
        Ok(())
    }
    fn ensure_worker(&mut self) -> Result<()> {
        if let Some(worker) = self.worker.as_mut() {
            if worker.child.try_wait()?.is_none() {
                self.policy.before_request(&self.config.model_path);
                return Ok(());
            }
            tracing::warn!(
                pid = worker.child.id(),
                "inference worker exited unexpectedly; launching replacement"
            );
            self.worker = None;
            self.mark_stopped();
        }
        let action = self.policy.before_request(&self.config.model_path);
        tracing::info!(?action, model = %self.config.model_path.display(), "launching disposable inference worker");
        let mut command = Command::new(&self.config.executable);
        command
            .arg("--runtime-dir")
            .arg(&self.config.runtime_dir)
            .arg("--model-path")
            .arg(&self.config.model_path)
            .arg("--log-path")
            .arg(&self.config.log_path)
            .arg("--log-level")
            .arg(self.config.log_level.as_str())
            .arg("--inference-device")
            .arg(self.config.inference_device.as_str())
            .arg("--idle-timeout-secs")
            .arg(self.config.idle_timeout.as_secs().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        match self.config.inference_device {
            InferenceDevice::Cpu => {
                command.env("PARAKEET_DEVICE", "cpu");
            }
            InferenceDevice::NvidiaGpu => {
                command.env_remove("PARAKEET_DEVICE");
            }
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.mark_stopped();
                return Err(error)
                    .with_context(|| format!("launching {}", self.config.executable.display()));
            }
        };
        self.pid_tracker.store(child.id(), Ordering::SeqCst);
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                self.mark_stopped();
                anyhow::bail!("worker stdin was not piped");
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => BufReader::new(stdout),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                self.mark_stopped();
                anyhow::bail!("worker stdout was not piped");
            }
        };
        let mut worker = WorkerHandle {
            child,
            stdin,
            stdout,
        };
        let handshake = (|| -> Result<()> {
            write_frame(&mut worker.stdin, &Frame::empty(MessageType::Hello))?;
            let response =
                read_frame(&mut worker.stdout).context("handshaking with inference worker")?;
            anyhow::ensure!(
                response.kind == MessageType::HelloAck,
                "inference worker rejected handshake"
            );
            Ok(())
        })();
        if let Err(error) = handshake {
            let pid = worker.child.id();
            tracing::warn!(pid, %error, "inference worker handshake failed; terminating child");
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            self.mark_stopped();
            return Err(error);
        }
        tracing::info!(pid = worker.child.id(), "inference worker ready");
        self.worker = Some(worker);
        Ok(())
    }
    fn discard_crashed_worker(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }
        self.mark_stopped();
    }
    fn mark_stopped(&mut self) {
        self.policy.stopped();
        self.pid_tracker.store(0, Ordering::SeqCst);
    }
}
impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown_now();
    }
}

pub fn nonzero_pid(tracker: &AtomicU32) -> Option<u32> {
    let pid = tracker.load(Ordering::SeqCst);
    (pid != 0).then_some(pid)
}

/// Requests a normal shutdown without making the caller wait behind a hung
/// inference request forever. If the supervisor mutex cannot complete its own
/// graceful path promptly, terminate only the tracked disposable child PID.
pub fn shutdown_shared(
    worker: Arc<Mutex<WorkerSupervisor>>,
    tracker: Arc<AtomicU32>,
    grace: Duration,
) -> Result<()> {
    let (done_tx, done_rx) = bounded::<Result<(), String>>(1);
    thread::spawn(move || {
        let result = worker
            .lock()
            .map_err(|_| "inference-worker mutex poisoned".to_owned())
            .and_then(|mut worker| worker.shutdown_now().map_err(|error| error.to_string()));
        let _ = done_tx.send(result);
    });
    match done_rx.recv_timeout(grace + SHARED_SHUTDOWN_OVERHEAD) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => anyhow::bail!(error),
        Err(RecvTimeoutError::Disconnected) => {
            anyhow::bail!("inference-worker shutdown coordinator disconnected")
        }
        Err(RecvTimeoutError::Timeout) => {
            let Some(pid) = nonzero_pid(&tracker) else {
                return Ok(());
            };
            tracing::warn!(
                pid,
                "inference-worker supervisor was blocked; force-terminating exact child PID"
            );
            force_terminate_pid(pid, SHARED_SHUTDOWN_RECOVERY)?;
            let _ = done_rx.recv_timeout(SHARED_SHUTDOWN_RECOVERY);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_launches_lazily_then_reuses() {
        let mut p = LifecyclePolicy::default();
        assert_eq!(
            p.before_request(Path::new("a.gguf")),
            LifecycleAction::Launch
        );
        assert_eq!(
            p.before_request(Path::new("a.gguf")),
            LifecycleAction::Reuse
        );
    }
    #[test]
    fn policy_replaces_after_model_switch() {
        let mut p = LifecyclePolicy::default();
        p.before_request(Path::new("a.gguf"));
        assert_eq!(
            p.before_request(Path::new("b.gguf")),
            LifecycleAction::ReplaceModel
        );
    }
    #[test]
    fn policy_requests_exit_after_idle_timeout() {
        let mut p = LifecyclePolicy::default();
        p.before_request(Path::new("a.gguf"));
        p.last_used = Some(Instant::now() - Duration::from_secs(10));
        assert_eq!(
            p.idle_action(Instant::now(), Duration::from_secs(5)),
            Some(LifecycleAction::ShutDownIdle)
        );
    }
    #[test]
    fn stopped_policy_launches_a_fresh_worker_next_time() {
        let mut p = LifecyclePolicy::default();
        p.before_request(Path::new("a.gguf"));
        p.stopped();
        assert_eq!(
            p.before_request(Path::new("a.gguf")),
            LifecycleAction::Launch
        );
    }
    #[test]
    fn zero_pid_is_hidden() {
        let tracker = AtomicU32::new(0);
        assert_eq!(nonzero_pid(&tracker), None);
        tracker.store(42, Ordering::SeqCst);
        assert_eq!(nonzero_pid(&tracker), Some(42));
    }
}
