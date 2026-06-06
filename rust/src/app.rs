use anyhow::{Context, Result};
use crossbeam_channel::{bounded, select, unbounded, Sender};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio;
use crate::config::AppConfig;
use crate::hotkey::{self, HotkeyEvent};
use crate::input::{foreground_window_id, WindowsTextSink};
use crate::transcript::Typist;
use crate::worker::{WorkerEvent, WorkerHandle};

struct Session {
    id: u64,
    target_window: isize,
    buffered_audio: VecDeque<Vec<i16>>,
    audio_frames_buffered: u64,
    audio_frames_sent: u64,
    audio_samples_sent: u64,
    last_audio_log: Instant,
}

pub fn run(config: AppConfig) -> Result<()> {
    config.validate()?;
    let (audio_tx, audio_rx) = bounded::<Vec<i16>>(512);
    let (_capture_handle, device_description) = start_audio(&config, audio_tx)?;
    tracing::info!(%device_description, "microphone capture started");
    tracing::debug!(?config, "loaded runtime config");

    let (hotkey_tx, hotkey_rx) = unbounded();
    let _hook = hotkey::spawn_capslock_hook(hotkey_tx)?;
    tracing::info!("CapsLock hook initialized");
    let (worker_tx, worker_rx) = unbounded();
    let (exit_tx, exit_rx) = bounded::<()>(1);
    ctrlc::set_handler(move || {
        let _ = exit_tx.try_send(());
    })
    .context("installing Ctrl+C handler")?;

    let typist = Typist::spawn(
        Arc::new(WindowsTextSink),
        config.typing_chunk_chars,
        Duration::from_millis(config.typing_interval_ms),
    );
    let mut state = State::new(config, worker_tx, typist);
    state.start_worker()?;
    println!("Uvox is running and warming the speech model. Wait for `worker status: ready`, then hold CapsLock to dictate. Release CapsLock to stop. Press Ctrl+C to exit.");

    loop {
        select! {
            recv(exit_rx) -> _ => {
                tracing::info!("Ctrl+C received; shutting down");
                break;
            },
            recv(hotkey_rx) -> message => if let Ok(event) = message {
                tracing::debug!(?event, "hotkey event received");
                state.handle_hotkey(event)?;
            },
            recv(audio_rx) -> message => if let Ok(frame) = message { state.handle_audio(frame)?; },
            recv(worker_rx) -> message => if let Ok(event) = message {
                tracing::debug!(?event, "worker event received");
                state.handle_worker_event(event)?;
            },
            default(Duration::from_millis(100)) => state.tick(),
        }
    }
    state.shutdown();
    Ok(())
}

fn start_audio(config: &AppConfig, tx: Sender<Vec<i16>>) -> Result<(audio::CaptureHandle, String)> {
    let description = if config.audio_device_contains.trim().is_empty() {
        "default input".to_owned()
    } else {
        config.audio_device_contains.clone()
    };
    Ok((
        audio::start_capture(&config.audio_device_contains, config.audio_gain, tx)?,
        description,
    ))
}

struct State {
    config: AppConfig,
    worker_events: Sender<WorkerEvent>,
    worker: Option<WorkerHandle>,
    worker_ready: bool,
    active: Option<Session>,
    next_session_id: u64,
    last_activity: Instant,
    typist: Typist,
}

impl State {
    fn new(config: AppConfig, worker_events: Sender<WorkerEvent>, typist: Typist) -> Self {
        Self {
            config,
            worker_events,
            worker: None,
            worker_ready: false,
            active: None,
            next_session_id: 1,
            last_activity: Instant::now(),
            typist,
        }
    }

    fn handle_hotkey(&mut self, event: HotkeyEvent) -> Result<()> {
        match event {
            HotkeyEvent::CapsLockDown if self.active.is_none() => {
                let id = self.next_session_id;
                self.next_session_id += 1;
                self.last_activity = Instant::now();
                self.typist.begin_session(id);
                self.active = Some(Session {
                    id,
                    target_window: foreground_window_id(),
                    buffered_audio: VecDeque::new(),
                    audio_frames_buffered: 0,
                    audio_frames_sent: 0,
                    audio_samples_sent: 0,
                    last_audio_log: Instant::now(),
                });
                tracing::info!(
                    session_id = id,
                    target_window = self
                        .active
                        .as_ref()
                        .map(|session| session.target_window)
                        .unwrap_or_default(),
                    "CapsLock down: session opened"
                );
                self.start_worker()?;
                if self.worker_ready {
                    tracing::debug!(
                        session_id = id,
                        "worker already ready; starting session immediately"
                    );
                    self.begin_active_session()?;
                } else {
                    tracing::debug!(
                        session_id = id,
                        "worker not ready; buffering microphone frames"
                    );
                }
            }
            HotkeyEvent::CapsLockUp => {
                if let Some(session) = self.active.take() {
                    self.last_activity = Instant::now();
                    self.typist.cancel(session.id);
                    if let Some(worker) = &self.worker {
                        let _ = worker.cancel_session(session.id);
                    }
                    tracing::info!(
                        session_id = session.id,
                        frames_buffered = session.audio_frames_buffered,
                        frames_sent = session.audio_frames_sent,
                        samples_sent = session.audio_samples_sent,
                        "CapsLock up: session cancelled"
                    );
                } else {
                    tracing::debug!("CapsLock up ignored because no session is active");
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn start_worker(&mut self) -> Result<()> {
        if self.worker.is_some() {
            return Ok(());
        }
        tracing::info!(
            backend = %self.config.worker_backend,
            lookahead_ms = self.config.lookahead_ms,
            "starting Python worker"
        );
        self.worker = Some(WorkerHandle::spawn(
            &self.config,
            self.worker_events.clone(),
        )?);
        self.worker_ready = false;
        Ok(())
    }

    fn begin_active_session(&mut self) -> Result<()> {
        let Some(session) = &mut self.active else {
            return Ok(());
        };
        let Some(worker) = &self.worker else {
            return Ok(());
        };
        let buffered_frames = session.buffered_audio.len();
        tracing::info!(
            session_id = session.id,
            buffered_frames,
            "starting worker session and draining buffered audio"
        );
        worker.start_session(session.id)?;
        while let Some(frame) = session.buffered_audio.pop_front() {
            session.audio_frames_sent += 1;
            session.audio_samples_sent += frame.len() as u64;
            worker.send_audio(session.id, &frame)?;
        }
        tracing::debug!(
            session_id = session.id,
            frames_sent = session.audio_frames_sent,
            samples_sent = session.audio_samples_sent,
            "buffer drain complete"
        );
        Ok(())
    }

    fn handle_audio(&mut self, frame: Vec<i16>) -> Result<()> {
        let Some(session) = &mut self.active else {
            return Ok(());
        };
        if self.worker_ready {
            if let Some(worker) = &self.worker {
                session.audio_frames_sent += 1;
                session.audio_samples_sent += frame.len() as u64;
                worker.send_audio(session.id, &frame)?;
            }
        } else {
            let max_frames = self.config.ring_buffer_secs * 50; // 20 ms frames
            if session.buffered_audio.len() >= max_frames {
                session.buffered_audio.pop_front();
            }
            session.audio_frames_buffered += 1;
            session.buffered_audio.push_back(frame);
        }
        if session.last_audio_log.elapsed() >= Duration::from_secs(1) {
            tracing::debug!(
                session_id = session.id,
                worker_ready = self.worker_ready,
                queued_frames = session.buffered_audio.len(),
                frames_buffered = session.audio_frames_buffered,
                frames_sent = session.audio_frames_sent,
                samples_sent = session.audio_samples_sent,
                "microphone audio flowing"
            );
            session.last_audio_log = Instant::now();
        }
        Ok(())
    }

    fn handle_worker_event(&mut self, event: WorkerEvent) -> Result<()> {
        match event {
            WorkerEvent::Loading => tracing::info!("worker status: loading model"),
            WorkerEvent::Ready => {
                tracing::info!("worker status: ready");
                self.worker_ready = true;
                self.begin_active_session()?;
            }
            WorkerEvent::Partial { session_id, text } => {
                if self.active.as_ref().map(|s| s.id) == Some(session_id) {
                    tracing::info!(session_id, %text, "partial transcript");
                } else {
                    tracing::debug!(session_id, %text, "ignored partial for inactive session");
                }
            }
            WorkerEvent::Commit { session_id, text } => {
                if let Some(session) = &self.active {
                    if session.id == session_id && foreground_window_id() == session.target_window {
                        tracing::info!(session_id, %text, "commit transcript; queueing text insertion");
                        self.typist.queue(session_id, session.target_window, text);
                    } else {
                        tracing::warn!(
                            session_id,
                            active_session_id = session.id,
                            target_window = session.target_window,
                            foreground_window = foreground_window_id(),
                            %text,
                            "ignored commit because session or foreground window changed"
                        );
                    }
                } else {
                    tracing::debug!(session_id, %text, "ignored commit because no session is active");
                }
            }
            WorkerEvent::Status(status) => tracing::info!(%status, "worker status"),
            WorkerEvent::Error(error) => tracing::error!(%error, "worker error"),
            WorkerEvent::Disconnected => {
                tracing::warn!("worker disconnected");
                self.worker = None;
                self.worker_ready = false;
            }
        }
        Ok(())
    }

    fn tick(&mut self) {
        if self.active.is_none()
            && self.worker.is_some()
            && self.last_activity.elapsed() >= Duration::from_secs(self.config.idle_timeout_secs)
        {
            tracing::info!("idle timeout reached; unloading CUDA worker");
            if let Some(worker) = self.worker.take() {
                worker.shutdown();
            }
            self.worker_ready = false;
        }
    }

    fn shutdown(&mut self) {
        if let Some(session) = self.active.take() {
            tracing::info!(
                session_id = session.id,
                "shutdown cancelling active session"
            );
            self.typist.cancel(session.id);
        }
        if let Some(worker) = self.worker.take() {
            tracing::info!("shutdown stopping Python worker");
            worker.shutdown();
        }
    }
}
