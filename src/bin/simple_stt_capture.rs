use anyhow::{Context, Result};
use clap::Parser;
use crossbeam_channel::{bounded, select, tick, unbounded, Sender};
use simple_stt::capture::audio::{self, AudioEvent};
use simple_stt::capture::inference_supervisor::{
    nonzero_pid, shutdown_shared, WorkerConfig, WorkerSupervisor,
};
use simple_stt::capture::ipc_server::{self, ControlRequest};
use simple_stt::capture::overlay::{OverlayHandle, OverlayPrimary};
use simple_stt::capture::state::ServiceState;
use simple_stt::common::shell_protocol::{NoticeLevel, ServiceEvent, ShellCommand, ShellResponse};
use simple_stt::config::AppConfig;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const MIN_RECORDING_SAMPLES: usize = 1_600; // 100 ms at 16 kHz.
const EVENT_HISTORY_LIMIT: usize = 512;

#[derive(Debug, Parser)]
#[command(
    name = "simple-stt-capture",
    about = "Persistent lightweight SimpleStt audio capture service"
)]
struct Args {
    #[arg(long)]
    token: String,
    #[arg(long)]
    state_file: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug)]
struct Recording {
    session_id: u64,
    samples: Vec<i16>,
    started: Instant,
}

#[derive(Debug)]
enum BackgroundResult {
    Transcript {
        session_id: u64,
        result: Result<String, String>,
    },
    ModelUnloaded {
        result: Result<(), String>,
    },
    WorkerConfigReplaced {
        result: Result<(), String>,
    },
    ModelLoaded,
    ModelWarmed {
        result: Result<(), String>,
    },
    ModelTested {
        result: Result<String, String>,
    },
    DownloadProgress {
        filename: String,
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadFinished {
        filename: String,
        result: Result<PathBuf, String>,
    },
}

struct ControlContext<'a> {
    config: &'a mut AppConfig,
    worker: &'a Arc<Mutex<WorkerSupervisor>>,
    worker_pid: &'a Arc<AtomicU32>,
    background_tx: &'a Sender<BackgroundResult>,
    overlay: &'a OverlayHandle,
    recording_active: &'a Arc<AtomicBool>,
    events: &'a mut EventBuffer,
    active: &'a mut Option<Recording>,
    transcribing: &'a mut HashSet<u64>,
    shutting_down: &'a mut bool,
}

#[derive(Debug, Default)]
struct EventBuffer {
    next_seq: u64,
    items: VecDeque<ServiceEvent>,
}
impl EventBuffer {
    fn push(&mut self, mut event: ServiceEvent) {
        self.next_seq += 1;
        event.seq = self.next_seq;
        self.items.push_back(event);
        while self.items.len() > EVENT_HISTORY_LIMIT {
            self.items.pop_front();
        }
    }
    fn after(&self, seq: u64) -> Vec<ServiceEvent> {
        self.items
            .iter()
            .filter(|event| event.seq > seq)
            .cloned()
            .collect()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(config) = &args.config {
        std::env::set_var("SIMPLE_STT_CONFIG", config);
    }
    let state_file = args
        .state_file
        .unwrap_or_else(AppConfig::service_state_path);
    let mut config = AppConfig::load()?;
    config.validate()?;
    simple_stt::logging::init_component(
        "capture",
        &AppConfig::capture_log_path(),
        &config.log_level,
    )?;
    tracing::info!(pid = std::process::id(), config = %AppConfig::config_path().display(), "capture service starting");

    let overlay = OverlayHandle::spawn()?;
    let recording_active = Arc::new(AtomicBool::new(false));
    let (frame_tx, frame_rx) = bounded::<Vec<i16>>(4096);
    let (audio_event_tx, audio_event_rx) = unbounded::<AudioEvent>();
    let _capture = audio::start_capture(
        &config.audio_device_contains,
        config.audio_gain,
        frame_tx,
        Some(overlay.level_cell()),
        Some(Arc::clone(&recording_active)),
        Some(audio_event_tx),
    )
    .context("starting CPAL microphone capture")?;

    let supervisor = WorkerSupervisor::new(worker_config(&config)?);
    let worker_pid = supervisor.pid_tracker();
    let worker = Arc::new(Mutex::new(supervisor));
    let (control_tx, control_rx) = unbounded::<ControlRequest>();
    let server = ipc_server::spawn(args.token, control_tx)?;
    ServiceState::new(server.address.clone()).save_atomic(&state_file)?;
    tracing::info!(address = %server.address, state_file = %state_file.display(), "capture service ready");

    let (background_tx, background_rx) = unbounded::<BackgroundResult>();
    let mut events = EventBuffer::default();
    events.push(ServiceEvent::simple("service_ready"));
    let timer = tick(Duration::from_millis(250));
    let mut active: Option<Recording> = None;
    let mut transcribing = HashSet::<u64>::new();
    let mut shutting_down = false;
    let idle_check_running = Arc::new(AtomicBool::new(false));

    while !shutting_down {
        select! {
            recv(frame_rx) -> message => if let (Some(recording), Ok(frame)) = (&mut active, message) { recording.samples.extend_from_slice(&frame); },
            recv(audio_event_rx) -> message => if let Ok(AudioEvent::StreamError(error)) = message {
                tracing::error!(%error, "audio service stream failure");
                overlay.notify_error("🎙 Audio service error — see log", Duration::from_secs(3));
                events.push(notice_event(NoticeLevel::Error, "Audio service error — see log"));
            },
            recv(background_rx) -> message => if let Ok(result) = message { handle_background(result, &overlay, &mut events, config.log_transcripts, active.is_some(), &mut transcribing); },
            recv(control_rx) -> message => if let Ok(request) = message {
                let response = handle_control(request.command, ControlContext {
                    config: &mut config,
                    worker: &worker,
                    worker_pid: &worker_pid,
                    background_tx: &background_tx,
                    overlay: &overlay,
                    recording_active: &recording_active,
                    events: &mut events,
                    active: &mut active,
                    transcribing: &mut transcribing,
                    shutting_down: &mut shutting_down,
                });
                let _ = request.reply.send(response);
            },
            recv(timer) -> _ => {
                if nonzero_pid(&worker_pid).is_some()
                    && !idle_check_running.swap(true, Ordering::SeqCst)
                {
                    let worker = Arc::clone(&worker);
                    let background_tx = background_tx.clone();
                    let idle_check_running = Arc::clone(&idle_check_running);
                    std::thread::spawn(move || {
                        match worker.lock().unwrap().shutdown_if_idle() {
                            Ok(true) => { let _ = background_tx.send(BackgroundResult::ModelUnloaded { result: Ok(()) }); }
                            Ok(false) => {}
                            Err(error) => { let _ = background_tx.send(BackgroundResult::ModelUnloaded { result: Err(error.to_string()) }); }
                        }
                        idle_check_running.store(false, Ordering::SeqCst);
                    });
                }
            }
        }
    }

    if let Some(recording) = active.take() {
        tracing::warn!(
            session_id = recording.session_id,
            "recording cancelled: capture service shutting down"
        );
    }
    overlay.hide();
    if let Err(error) = shutdown_shared(
        Arc::clone(&worker),
        Arc::clone(&worker_pid),
        Duration::from_millis(config.worker_shutdown_grace_ms),
    ) {
        tracing::error!(%error, "inference-worker shutdown failed while capture service was stopping");
    }
    if state_file.exists() {
        fs::remove_file(&state_file).ok();
    }
    tracing::info!(pid = std::process::id(), "capture service stopped");
    Ok(())
}

fn handle_control(command: ShellCommand, context: ControlContext<'_>) -> ShellResponse {
    let ControlContext {
        config,
        worker,
        worker_pid,
        background_tx,
        overlay,
        recording_active,
        events,
        active,
        transcribing,
        shutting_down,
    } = context;
    match command {
        ShellCommand::Ping => {
            let mut response = ShellResponse::ok("pong");
            response
                .values
                .insert("service_pid".into(), std::process::id().to_string());
            if let Some(pid) = nonzero_pid(worker_pid) {
                response.values.insert("worker_pid".into(), pid.to_string());
            }
            response
        }
        ShellCommand::StartRecording { session_id } => {
            if active.is_some() {
                return ShellResponse::error("a recording is already active");
            }
            recording_active.store(true, Ordering::Relaxed);
            *active = Some(Recording {
                session_id,
                samples: Vec::new(),
                started: Instant::now(),
            });
            overlay.start_recording(0);
            let mut event = ServiceEvent::simple("recording_started");
            event.session_id = Some(session_id);
            events.push(event);
            if nonzero_pid(worker_pid).is_none() {
                overlay.notify_info("🎙 Loading speech model…", None);
                let mut loading = ServiceEvent::simple("model_loading");
                loading.session_id = Some(session_id);
                events.push(loading);
                let worker = Arc::clone(worker);
                let tx = background_tx.clone();
                std::thread::spawn(move || {
                    let progress_tx = tx.clone();
                    let result = worker
                        .lock()
                        .map_err(|_| "inference-worker mutex poisoned".to_owned())
                        .and_then(|mut worker| {
                            worker
                                .warm_up(|| {
                                    let _ = progress_tx.send(BackgroundResult::ModelLoaded);
                                })
                                .map_err(|error| error.to_string())
                        });
                    let _ = tx.send(BackgroundResult::ModelWarmed { result });
                });
            }
            tracing::info!(session_id, "recording start");
            ShellResponse::ok("recording started")
        }
        ShellCommand::StopRecording { session_id } => {
            let Some(recording) = active.take() else {
                return ShellResponse::error("no recording is active");
            };
            if recording.session_id != session_id {
                *active = Some(recording);
                return ShellResponse::error("recording session id mismatch");
            }
            recording_active.store(false, Ordering::Relaxed);
            let duration_ms = recording.started.elapsed().as_millis();
            let samples = recording.samples;
            tracing::info!(
                session_id,
                duration_ms,
                samples = samples.len(),
                "recording stop"
            );
            if samples.len() < MIN_RECORDING_SAMPLES {
                overlay.notify_warning("🎙 Recording too short", Duration::from_secs(2));
                restore_overlay_work_state(overlay, false, !transcribing.is_empty());
                events.push(notice_event_for_session(
                    NoticeLevel::Warning,
                    "Recording too short",
                    session_id,
                ));
                return ShellResponse::ok("recording rejected as too short");
            }
            transcribing.insert(session_id);
            overlay.set_primary(OverlayPrimary::Transcribing);
            let mut event = ServiceEvent::simple("transcribing");
            event.session_id = Some(session_id);
            events.push(event);
            if nonzero_pid(worker_pid).is_none() {
                overlay.notify_info("🎙 Loading speech model…", None);
                let mut loading = ServiceEvent::simple("model_loading");
                loading.session_id = Some(session_id);
                events.push(loading);
            }
            let worker = Arc::clone(worker);
            let tx = background_tx.clone();
            std::thread::spawn(move || {
                let result = worker
                    .lock()
                    .unwrap()
                    .transcribe_pcm(session_id, &samples)
                    .map_err(|error| error.to_string());
                let _ = tx.send(BackgroundResult::Transcript { session_id, result });
            });
            ShellResponse::ok("transcription queued")
        }
        ShellCommand::PollEvents { after_seq } => {
            let mut response = ShellResponse::ok("events");
            response.events = events.after(after_seq);
            response
                .values
                .insert("latest_seq".into(), events.next_seq.to_string());
            response
        }
        ShellCommand::ReloadConfig => match AppConfig::load() {
            Ok(next) => {
                let restart_audio = next.audio_device_contains != config.audio_device_contains
                    || (next.audio_gain - config.audio_gain).abs() > f32::EPSILON
                    || next.log_level != config.log_level;
                match worker_config(&next) {
                    Ok(next_worker) => {
                        *config = next;
                        let worker = Arc::clone(worker);
                        let tx = background_tx.clone();
                        std::thread::spawn(move || {
                            let result = worker
                                .lock()
                                .map_err(|_| "inference-worker mutex poisoned".to_owned())
                                .and_then(|mut worker| {
                                    worker
                                        .replace_config(next_worker)
                                        .map_err(|error| error.to_string())
                                });
                            let _ = tx.send(BackgroundResult::WorkerConfigReplaced { result });
                        });
                        tracing::info!(
                            restart_audio,
                            "configuration reload accepted; worker changes queued"
                        );
                        let mut response = ShellResponse::ok("configuration reload queued");
                        response
                            .values
                            .insert("restart_audio_service".into(), restart_audio.to_string());
                        response
                    }
                    Err(error) => ShellResponse::error(error.to_string()),
                }
            }
            Err(error) => ShellResponse::error(error.to_string()),
        },
        ShellCommand::UnloadModel => {
            let worker = Arc::clone(worker);
            let tracker = Arc::clone(worker_pid);
            let tx = background_tx.clone();
            let grace = Duration::from_millis(config.worker_shutdown_grace_ms);
            std::thread::spawn(move || {
                let result =
                    shutdown_shared(worker, tracker, grace).map_err(|error| error.to_string());
                let _ = tx.send(BackgroundResult::ModelUnloaded { result });
            });
            ShellResponse::ok("speech-model worker shutdown requested")
        }
        ShellCommand::TestModel => {
            let audio = simple_stt::models::smoke_audio_path();
            if let Err(error) = simple_stt::models::ensure_smoke_audio(&audio) {
                return ShellResponse::error(error.to_string());
            }
            if nonzero_pid(worker_pid).is_none() {
                overlay.notify_info("🎙 Loading speech model…", None);
                events.push(ServiceEvent::simple("model_loading"));
            }
            let worker = Arc::clone(worker);
            let tx = background_tx.clone();
            std::thread::spawn(move || {
                let result = worker
                    .lock()
                    .unwrap()
                    .transcribe_wav(0, &audio)
                    .map_err(|error| error.to_string());
                let _ = tx.send(BackgroundResult::ModelTested { result });
            });
            ShellResponse::ok("model test queued")
        }
        ShellCommand::DownloadModel { filename } => {
            let config = config.clone();
            let tx = background_tx.clone();
            let progress_tx = tx.clone();
            let filename_for_thread = filename.clone();
            std::thread::spawn(move || {
                let result = simple_stt::models::download_model(
                    &config,
                    &filename_for_thread,
                    |downloaded, total| {
                        let _ = progress_tx.send(BackgroundResult::DownloadProgress {
                            filename: filename_for_thread.clone(),
                            downloaded,
                            total,
                        });
                    },
                )
                .map_err(|error| error.to_string());
                let _ = tx.send(BackgroundResult::DownloadFinished {
                    filename: filename_for_thread,
                    result,
                });
            });
            ShellResponse::ok("model download queued")
        }
        ShellCommand::ListInputs => match audio::list_input_devices() {
            Ok(devices) => {
                let mut response = ShellResponse::ok("microphone devices");
                for (index, device) in devices.into_iter().enumerate() {
                    response.values.insert(format!("input.{index:03}"), device);
                }
                response
            }
            Err(error) => ShellResponse::error(error.to_string()),
        },
        ShellCommand::ListModels => {
            let mut response = ShellResponse::ok("cached models");
            response.values.insert(
                "recommended_model".into(),
                simple_stt::models::recommended_model_for_device(&config.inference_device).into(),
            );
            for (index, model) in simple_stt::models::installed_models(config)
                .into_iter()
                .enumerate()
            {
                response.values.insert(
                    format!("installed_model.{index:03}"),
                    format!("{}|{}|{}|{}", model.file, model.size_mb, model.recommended, model.quant),
                );
            }
            for (index, model) in simple_stt::models::downloadable_models(config)
                .into_iter()
                .enumerate()
            {
                response.values.insert(
                    format!("catalog_model.{index:03}"),
                    format!("{}|{}|{}|{}", model.file, model.size_mb, model.recommended, model.quant),
                );
            }
            response
        }
        ShellCommand::RefreshModels => match simple_stt::models::refresh_catalog_cache() {
            Ok(files) => {
                let mut response = ShellResponse::ok("model catalog refreshed");
                response
                    .values
                    .insert("count".into(), files.len().to_string());
                response
            }
            Err(error) => ShellResponse::error(error.to_string()),
        },
        ShellCommand::ShowNotice { level, text } => {
            match level {
                NoticeLevel::Info => overlay.notify_info(&text, Some(Duration::from_secs(2))),
                NoticeLevel::Warning => overlay.notify_warning(&text, Duration::from_secs(3)),
                NoticeLevel::Error => overlay.notify_error(&text, Duration::from_secs(3)),
            }
            ShellResponse::ok("notice shown")
        }
        ShellCommand::Shutdown => {
            recording_active.store(false, Ordering::Relaxed);
            *shutting_down = true;
            ShellResponse::ok("capture service shutting down")
        }
    }
}

fn handle_background(
    result: BackgroundResult,
    overlay: &OverlayHandle,
    events: &mut EventBuffer,
    log_transcripts: bool,
    active_recording: bool,
    transcribing: &mut HashSet<u64>,
) {
    match result {
        BackgroundResult::Transcript { session_id, result } => {
            transcribing.remove(&session_id);
            let has_pending_transcript = !transcribing.is_empty();
            match result {
                Ok(text) if text.trim().is_empty() => {
                    overlay.notify_warning("🎙 No speech detected", Duration::from_secs(2));
                    restore_overlay_work_state(overlay, active_recording, has_pending_transcript);
                    events.push(notice_event_for_session(
                        NoticeLevel::Warning,
                        "No speech detected",
                        session_id,
                    ));
                }
                Ok(text) => {
                    if log_transcripts {
                        tracing::debug!(session_id, transcript = %text, "diagnostic transcript logging enabled");
                    }
                    restore_overlay_after_success(
                        overlay,
                        active_recording,
                        has_pending_transcript,
                    );
                    let mut event = ServiceEvent::simple("transcript");
                    event.session_id = Some(session_id);
                    event.text = text;
                    tracing::info!(
                        session_id,
                        transcript_chars = event.text.chars().count(),
                        "transcript ready"
                    );
                    events.push(event);
                }
                Err(error) => {
                    tracing::error!(session_id, %error, "speech engine failed");
                    overlay.notify_error("🎙 Speech engine failed — see log", Duration::from_secs(3));
                    restore_overlay_work_state(overlay, active_recording, has_pending_transcript);
                    events.push(notice_event_for_session(
                        NoticeLevel::Error,
                        "Speech engine failed — see log",
                        session_id,
                    ));
                }
            }
        }
        BackgroundResult::ModelUnloaded { result } => match result {
            Ok(()) => {
                overlay.notify_info("🎙 Speech model unloaded", Some(Duration::from_secs(2)));
            }
            Err(error) => tracing::warn!(%error, "worker shutdown failed"),
        },
        BackgroundResult::WorkerConfigReplaced { result } => match result {
            Ok(()) => tracing::info!("disposable inference-worker configuration applied"),
            Err(error) => {
                tracing::error!(%error, "applying inference-worker configuration failed");
                events.push(notice_event(
                    NoticeLevel::Error,
                    "Speech-worker settings failed — see log",
                ));
            }
        },
        BackgroundResult::ModelLoaded => {
            tracing::info!("speech model loaded; priming inference engine");
            overlay.notify_info("🎙 Speech model loaded - warming up...", None);
            events.push(ServiceEvent::simple("model_loaded"));
        }
        BackgroundResult::ModelWarmed { result } => match result {
            Ok(()) => {
                tracing::info!("speech model warmed while recording");
                overlay.notify_info("🎙 Speech model ready", Some(Duration::from_secs(2)));
                events.push(ServiceEvent::simple("model_ready"));
            }
            Err(error) => {
                tracing::warn!(%error, "speech-model warm-up failed; transcription will retry");
                overlay.notify_warning(
                    "🎙 Speech model load failed — transcription will retry",
                    Duration::from_secs(3),
                );
            }
        },
        BackgroundResult::ModelTested { result } => match result {
            Ok(text) => {
                overlay.notify_info("🎙 Model test passed", Some(Duration::from_secs(2)));
                let mut event = ServiceEvent::simple("model_test_complete");
                event.text = format!("Model test passed ({} characters)", text.chars().count());
                events.push(event);
            }
            Err(error) => {
                tracing::error!(%error, "model test failed");
                overlay.notify_error("🎙 Model test failed — see log", Duration::from_secs(3));
                events.push(notice_event(
                    NoticeLevel::Error,
                    "Model test failed — see log",
                ));
            }
        },
        BackgroundResult::DownloadProgress {
            filename,
            downloaded,
            total,
        } => {
            let mut event = ServiceEvent::simple("model_download_progress");
            event.values.insert("filename".into(), filename);
            event
                .values
                .insert("downloaded".into(), downloaded.to_string());
            if let Some(total) = total {
                event.values.insert("total".into(), total.to_string());
            }
            events.push(event);
        }
        BackgroundResult::DownloadFinished { filename, result } => match result {
            Ok(path) => {
                let mut event = ServiceEvent::simple("model_download_complete");
                event.values.insert("filename".into(), filename);
                event
                    .values
                    .insert("path".into(), path.display().to_string());
                events.push(event);
            }
            Err(error) => {
                tracing::error!(%error, "model download failed");
                events.push(notice_event(
                    NoticeLevel::Error,
                    "Model download failed — see log",
                ));
            }
        },
    }
}
fn restore_overlay_after_success(
    overlay: &OverlayHandle,
    active_recording: bool,
    has_pending_transcript: bool,
) {
    if active_recording {
        overlay.set_primary(OverlayPrimary::Recording);
    } else if has_pending_transcript {
        overlay.set_primary(OverlayPrimary::Transcribing);
    } else {
        overlay.hide();
    }
}

fn overlay_primary_for_work(
    active_recording: bool,
    has_pending_transcript: bool,
) -> OverlayPrimary {
    if active_recording {
        OverlayPrimary::Recording
    } else if has_pending_transcript {
        OverlayPrimary::Transcribing
    } else {
        OverlayPrimary::Hidden
    }
}

fn restore_overlay_work_state(
    overlay: &OverlayHandle,
    active_recording: bool,
    has_pending_transcript: bool,
) {
    overlay.set_primary(overlay_primary_for_work(
        active_recording,
        has_pending_transcript,
    ));
}

fn notice_event(level: NoticeLevel, text: &str) -> ServiceEvent {
    let mut event = ServiceEvent::simple("notice");
    event.level = level;
    event.text = text.into();
    event
}
fn notice_event_for_session(level: NoticeLevel, text: &str, session_id: u64) -> ServiceEvent {
    let mut event = notice_event(level, text);
    event.session_id = Some(session_id);
    event
}

fn worker_config(config: &AppConfig) -> Result<WorkerConfig> {
    Ok(WorkerConfig {
        executable: sibling_executable("simple-stt-infer")?,
        runtime_dir: config.parakeet_runtime_dir_path(),
        model_path: config.selected_model_path(),
        log_path: AppConfig::infer_log_path(),
        log_level: config.log_level.clone(),
        inference_device: config.inference_device.clone(),
        idle_timeout: Duration::from_secs(config.idle_worker_timeout_secs),
        shutdown_grace: Duration::from_millis(config.worker_shutdown_grace_ms),
    })
}
fn sibling_executable(stem: &str) -> Result<PathBuf> {
    let current = std::env::current_exe().context("resolving current executable")?;
    let parent = current
        .parent()
        .context("capture executable has no parent directory")?;
    Ok(parent.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_buffer_assigns_monotonic_sequences() {
        let mut events = EventBuffer::default();
        events.push(ServiceEvent::simple("first"));
        events.push(ServiceEvent::simple("second"));
        assert_eq!(
            events
                .after(0)
                .iter()
                .map(|event| event.seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            events
                .after(1)
                .iter()
                .map(|event| event.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["second"]
        );
    }

    #[test]
    fn event_buffer_discards_oldest_items_at_the_history_limit() {
        let mut events = EventBuffer::default();
        for index in 0..=EVENT_HISTORY_LIMIT {
            events.push(ServiceEvent::simple(format!("event-{index}")));
        }
        assert_eq!(events.items.len(), EVENT_HISTORY_LIMIT);
        assert_eq!(events.items.front().unwrap().seq, 2);
    }

    #[test]
    fn newer_overlay_work_survives_older_transcript_completion() {
        assert_eq!(
            overlay_primary_for_work(true, false),
            OverlayPrimary::Recording
        );
        assert_eq!(
            overlay_primary_for_work(false, true),
            OverlayPrimary::Transcribing
        );
        assert_eq!(
            overlay_primary_for_work(false, false),
            OverlayPrimary::Hidden
        );
    }
}
