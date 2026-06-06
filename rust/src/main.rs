#[cfg(not(windows))]
use anyhow::bail;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uvox::config::AppConfig;

#[derive(Parser, Debug)]
#[command(
    name = "uvox",
    version,
    about = "Native CUDA Parakeet hold-to-record dictation for Windows"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    /// Hold CapsLock to record, release to transcribe and type.
    Run,
    /// Open the lightweight settings file editor.
    Settings,
    /// Print the JSON config path and current config.
    ConfigShow,
    /// Reset the JSON config to defaults.
    ConfigReset,
    /// List microphone input devices.
    ListInputs,
    /// Test native Parakeet against a WAV file.
    TranscribeFile {
        #[arg(long)]
        audio: PathBuf,
    },
    /// Save an agent-friendly PNG rendering of a UI surface.
    UiScreenshot {
        #[arg(long, value_enum)]
        surface: UiSurfaceArg,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        section: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load().ok();
    uvox::logging::init(config.as_ref())?;
    match cli.command {
        CommandKind::Run => run_app(),
        CommandKind::Settings => show_settings(),
        CommandKind::ConfigShow => config_show(),
        CommandKind::ConfigReset => config_reset(),
        CommandKind::ListInputs => list_inputs(),
        CommandKind::TranscribeFile { audio } => transcribe_file(audio),
        CommandKind::UiScreenshot {
            surface,
            output,
            section,
        } => ui_screenshot(surface, output, section.as_deref()),
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum UiSurfaceArg {
    Settings,
    Overlay,
    OverlayDesktop,
}

fn config_show() -> Result<()> {
    let config = AppConfig::load()?;
    println!("Config path: {}", AppConfig::config_path().display());
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

fn config_reset() -> Result<()> {
    let config = AppConfig::default();
    config.save()?;
    println!("Reset config: {}", AppConfig::config_path().display());
    Ok(())
}

#[cfg(windows)]
fn show_settings() -> Result<()> {
    uvox::gui::show_settings()
}

#[cfg(not(windows))]
fn show_settings() -> Result<()> {
    bail!("The settings helper targets Windows")
}

#[cfg(windows)]
fn list_inputs() -> Result<()> {
    for (index, name) in uvox::audio::list_input_devices()?.into_iter().enumerate() {
        println!("{index}: {name}");
    }
    Ok(())
}

#[cfg(not(windows))]
fn list_inputs() -> Result<()> {
    bail!("Microphone enumeration is implemented for Windows")
}

#[cfg(windows)]
struct Recording {
    session_id: u64,
    target_window: isize,
    samples: Vec<i16>,
}

#[cfg(windows)]
fn run_app() -> Result<()> {
    use crossbeam_channel::{bounded, select, unbounded};
    use std::sync::Arc;
    use uvox::hotkey::{self, HotkeyEvent};
    use uvox::input::{foreground_window_id, WindowsTextSink};
    use uvox::overlay::OverlayHandle;
    use uvox::parakeet_native::ParakeetNative;
    use uvox::transcript::Typist;
    use uvox::tray::{TrayCommand, TrayHandle, TrayStatus};

    let mut config = AppConfig::load()?;
    config.validate()?;
    config.validate_parakeet_files()?;
    hotkey::set_enabled(config.hotkey_enabled);
    if config.capslock_always_off {
        uvox::input::set_capslock_state(false);
    }

    let (audio_tx, audio_rx) = bounded::<Vec<i16>>(4096);
    let _capture =
        uvox::audio::start_capture(&config.audio_device_contains, config.audio_gain, audio_tx)?;
    let (hotkey_tx, hotkey_rx) = unbounded();
    let _hook = hotkey::spawn_capslock_hook(hotkey_tx)?;
    let (tray_tx, tray_rx) = unbounded();
    let tray = TrayHandle::spawn(tray_tx)?;
    let overlay = OverlayHandle::spawn()?;
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

    let mut parakeet: Option<ParakeetNative> = None;
    let mut active: Option<Recording> = None;
    let mut next_session_id = 1_u64;
    let mut last_activity = Instant::now();
    let mut last_config_mtime = config_mtime();

    println!("Uvox is running.");
    println!("Hold CapsLock to record. Release CapsLock to transcribe with native CUDA Parakeet and type.");
    println!("Press Ctrl+C to exit.");

    loop {
        select! {
            recv(exit_rx) -> _ => break,
            recv(hotkey_rx) -> message => match message {
                Ok(HotkeyEvent::CapsLockDown) if active.is_none() => {
                    if !config.hotkey_enabled {
                        continue;
                    }
                    if config.capslock_always_off {
                        uvox::input::set_capslock_state(false);
                    }
                    let session_id = next_session_id;
                    next_session_id += 1;
                    let target_window = foreground_window_id();
                    overlay.show(target_window);
                    tray.set_status(TrayStatus::Recording);
                    typist.begin_session(session_id);
                    active = Some(Recording {
                        session_id,
                        target_window,
                        samples: Vec::new(),
                    });
                    if parakeet.is_none() {
                        tracing::info!(
                            runtime_dir = %config.parakeet_runtime_dir_path().display(),
                            model = %config.parakeet_model_path().display(),
                            "loading native CUDA Parakeet runtime"
                        );
                        parakeet = Some(ParakeetNative::load_from_config(&config)?);
                        tracing::info!("native CUDA Parakeet runtime ready");
                    }
                    last_activity = Instant::now();
                    tracing::info!(session_id, target_window, "CapsLock down: recording");
                }
                Ok(HotkeyEvent::CapsLockUp) => {
                    if config.capslock_always_off {
                        uvox::input::set_capslock_state(false);
                    }
                    if let Some(recording) = active.take() {
                        overlay.hide();
                        tray.set_status(TrayStatus::Transcribing);
                        let samples = recording.samples.len();
                        tracing::info!(session_id = recording.session_id, samples, "CapsLock up: transcribing");
                        if samples < 1_600 {
                            tracing::warn!(session_id = recording.session_id, samples, "ignored very short recording");
                            typist.cancel(recording.session_id);
                            tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                            continue;
                        }
                        let Some(engine) = &parakeet else {
                            tracing::error!(session_id = recording.session_id, "Parakeet runtime was not loaded");
                            typist.cancel(recording.session_id);
                            tray.set_status(TrayStatus::Error);
                            continue;
                        };
                        match engine.transcribe_pcm16_16k(&recording.samples) {
                            Ok(text) if !text.trim().is_empty() => {
                                let text = text.trim();
                                tracing::info!(session_id = recording.session_id, text, "native Parakeet transcript ready");
                                let foreground = foreground_window_id();
                                if foreground == recording.target_window {
                                    typist.queue(recording.session_id, recording.target_window, format!("{text} "));
                                } else {
                                    tracing::warn!(
                                        session_id = recording.session_id,
                                        target_window = recording.target_window,
                                        foreground_window = foreground,
                                        "not typing transcript because focus changed"
                                    );
                                    typist.cancel(recording.session_id);
                                }
                            }
                            Ok(_) => {
                                tracing::warn!(session_id = recording.session_id, "native Parakeet returned empty transcript");
                                typist.cancel(recording.session_id);
                            }
                            Err(error) => {
                                tracing::error!(session_id = recording.session_id, %error, "native Parakeet transcription failed");
                                typist.cancel(recording.session_id);
                                tray.set_status(TrayStatus::Error);
                            }
                        }
                        last_activity = Instant::now();
                        tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            },
            recv(audio_rx) -> message => {
                if let (Some(recording), Ok(frame)) = (&mut active, message) {
                    overlay.set_level(rms_level(&frame));
                    recording.samples.extend_from_slice(&frame);
                    if recording.samples.len() % 16_000 == 0 {
                        tracing::debug!(
                            session_id = recording.session_id,
                            seconds = recording.samples.len() as f32 / 16_000.0,
                            "recording audio"
                        );
                    }
                }
            },
            recv(tray_rx) -> message => match message {
                Ok(TrayCommand::OpenSettings) => {
                    if let Err(error) = uvox::gui::show_settings_on_event_loop() {
                        tracing::error!(%error, "settings window failed");
                    }
                }
                Ok(TrayCommand::ToggleHotkey) => {
                    config.hotkey_enabled = !config.hotkey_enabled;
                    hotkey::set_enabled(config.hotkey_enabled);
                    let _ = config.save();
                    tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                    tracing::info!(enabled = config.hotkey_enabled, "hotkey setting changed from tray");
                }
                Ok(TrayCommand::ReloadConfig) => {
                    match AppConfig::load() {
                        Ok(new_config) => {
                            config = new_config;
                            hotkey::set_enabled(config.hotkey_enabled);
                            if config.capslock_always_off {
                                uvox::input::set_capslock_state(false);
                            }
                            parakeet = None;
                            tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                            tracing::info!("configuration reloaded");
                        }
                        Err(error) => {
                            tray.set_status(TrayStatus::Error);
                            tracing::error!(%error, "configuration reload failed");
                        }
                    }
                }
                Ok(TrayCommand::OpenLog) => {
                    if let Err(error) = uvox::gui::open_latest_log() {
                        tracing::error!(%error, "opening latest log failed");
                    }
                }
                Ok(TrayCommand::TestModel) => {
                    let audio = uvox::config::repo_root().join("tests").join("fixtures").join("parakeet-smoke.wav");
                    match uvox::models::smoke_test_model(&config.parakeet_runtime_dir_path(), &config.parakeet_model_path(), &audio) {
                        Ok(text) => tracing::info!(text, "tray model test passed"),
                        Err(error) => {
                            tray.set_status(TrayStatus::Error);
                            tracing::error!(%error, "tray model test failed");
                        }
                    }
                }
                Ok(TrayCommand::Exit) => break,
                Err(_) => break,
            },
            default(Duration::from_millis(100)) => {
                let current_mtime = config_mtime();
                if current_mtime != last_config_mtime {
                    last_config_mtime = current_mtime;
                    match AppConfig::load() {
                        Ok(new_config) => {
                            config = new_config;
                            hotkey::set_enabled(config.hotkey_enabled);
                            if config.capslock_always_off {
                                uvox::input::set_capslock_state(false);
                            }
                            tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                            tracing::info!("configuration file changed; reloaded live settings");
                        }
                        Err(error) => {
                            tracing::error!(%error, "configuration file changed but reload failed");
                            tray.set_status(TrayStatus::Error);
                        }
                    }
                }
                if active.is_none()
                    && parakeet.is_some()
                    && last_activity.elapsed() >= Duration::from_secs(config.idle_timeout_secs)
                {
                    tracing::info!("idle timeout reached; unloading native CUDA Parakeet runtime");
                    parakeet = None;
                }
            },
        }
    }

    if let Some(recording) = active {
        typist.cancel(recording.session_id);
    }
    overlay.hide();
    tray.shutdown();
    Ok(())
}

#[cfg(not(windows))]
fn run_app() -> Result<()> {
    bail!("Uvox targets Windows because it uses the Windows keyboard hook and SendInput")
}

#[cfg(windows)]
fn transcribe_file(audio: PathBuf) -> Result<()> {
    let config = AppConfig::load()?;
    config.validate()?;
    config.validate_parakeet_files()?;
    let engine = uvox::parakeet_native::ParakeetNative::load_from_config(&config)?;
    let text = engine.transcribe_wav(&audio)?;
    println!("{text}");
    Ok(())
}

#[cfg(not(windows))]
fn transcribe_file(_audio: PathBuf) -> Result<()> {
    bail!("Native Parakeet transcription is implemented for Windows")
}

fn ui_screenshot(surface: UiSurfaceArg, output: PathBuf, section: Option<&str>) -> Result<()> {
    let surface = match surface {
        UiSurfaceArg::Settings => uvox::screenshots::UiSurface::Settings,
        UiSurfaceArg::Overlay => uvox::screenshots::UiSurface::Overlay,
        UiSurfaceArg::OverlayDesktop => uvox::screenshots::UiSurface::OverlayDesktop,
    };
    uvox::screenshots::save(surface, &output, section)?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn rms_level(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum = frame
        .iter()
        .map(|sample| {
            let value = *sample as f32 / 32768.0;
            value * value
        })
        .sum::<f32>();
    let rms = (sum / frame.len() as f32).sqrt();
    (rms * 14.0).powf(0.72).clamp(0.0, 1.0)
}

fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(AppConfig::config_path())
        .ok()
        .and_then(|meta| meta.modified().ok())
}
