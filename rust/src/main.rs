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
    #[command(hide = true)]
    TooltipBench {
        #[arg(long, default_value_t = 500)]
        iterations: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load().ok();
    if matches!(cli.command, CommandKind::Settings) {
        uvox::logging::init_append(config.as_ref())?;
    } else {
        uvox::logging::init(config.as_ref())?;
    }
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
        CommandKind::TooltipBench { iterations } => tooltip_bench(iterations),
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
    let mut record_hotkey = hotkey::HotkeySpec::parse(&config.record_hotkey)?;
    hotkey::set_enabled(config.hotkey_enabled);
    if config.capslock_always_off {
        uvox::input::set_capslock_state(false);
    }

    let overlay = OverlayHandle::spawn()?;
    let (audio_tx, audio_rx) = bounded::<Vec<i16>>(4096);
    let _capture = uvox::audio::start_capture_with_level(
        &config.audio_device_contains,
        config.audio_gain,
        audio_tx,
        Some(overlay.level_cell()),
    )?;
    let (hotkey_tx, hotkey_rx) = unbounded();
    let _hook = hotkey::spawn_hotkey_hook(record_hotkey.clone(), hotkey_tx)?;
    let (tray_tx, tray_rx) = unbounded();
    let tray = TrayHandle::spawn(tray_tx)?;
    // Cross-process reload signal: the settings subprocess writes a sentinel
    // file when config is saved. We forward that notification into this loop.
    let reload_rx = uvox::reload_event::create_reload_channel()?;
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
    let mut last_config_fingerprint = config_fingerprint();

    println!("Uvox is running.");
    println!(
        "Hold {} to record. Release the final key to transcribe with native CUDA Parakeet and type.",
        record_hotkey.label
    );
    println!("Press Ctrl+C to exit.");

    loop {
        select! {
            recv(exit_rx) -> _ => break,
            recv(hotkey_rx) -> message => match message {
                Ok(HotkeyEvent::HotkeyDown) if active.is_none() => {
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
                    } else if !parakeet.as_ref().unwrap().is_context_loaded() {
                        tracing::info!(
                            runtime_dir = %config.parakeet_runtime_dir_path().display(),
                            model = %config.parakeet_model_path().display(),
                            "reloading native CUDA Parakeet context"
                        );
                        parakeet.as_mut().unwrap().load_context(&config.parakeet_model_path())?;
                        tracing::info!("native CUDA Parakeet runtime ready");
                    }
                    last_activity = Instant::now();
                    tracing::info!(session_id, target_window, hotkey = record_hotkey.label, "hotkey down: recording");
                }
                Ok(HotkeyEvent::HotkeyUp) => {
                    if config.capslock_always_off {
                        uvox::input::set_capslock_state(false);
                    }
                    if let Some(recording) = active.take() {
                        overlay.hide();
                        tray.set_status(TrayStatus::Transcribing);
                        let samples = recording.samples.len();
                        tracing::info!(session_id = recording.session_id, samples, hotkey = record_hotkey.label, "hotkey up: transcribing");
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
                            let runtime_changed =
                                config.parakeet_runtime_dir_path() != new_config.parakeet_runtime_dir_path();
                            let model_changed =
                                config.parakeet_model_path() != new_config.parakeet_model_path();
                            config = new_config;
                            record_hotkey = hotkey::HotkeySpec::parse(&config.record_hotkey)?;
                            hotkey::set_record_hotkey(record_hotkey.clone());
                            hotkey::set_enabled(config.hotkey_enabled);
                            if config.capslock_always_off {
                                uvox::input::set_capslock_state(false);
                            }
                            if runtime_changed {
                                parakeet = None;
                            } else if model_changed {
                                if let Some(engine) = &mut parakeet {
                                    engine.unload_context();
                                }
                            }
                            tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                            tracing::info!(hotkey = %record_hotkey.label, enabled = config.hotkey_enabled, "configuration reloaded");
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
            recv(reload_rx) -> _ => {
                tracing::info!("settings subprocess signalled config reload");
                match AppConfig::load() {
                    Ok(new_config) => {
                        let runtime_changed =
                            config.parakeet_runtime_dir_path() != new_config.parakeet_runtime_dir_path();
                        let model_changed =
                            config.parakeet_model_path() != new_config.parakeet_model_path();
                        config = new_config;
                        record_hotkey = hotkey::HotkeySpec::parse(&config.record_hotkey)?;
                        hotkey::set_record_hotkey(record_hotkey.clone());
                        hotkey::set_enabled(config.hotkey_enabled);
                        if config.capslock_always_off {
                            uvox::input::set_capslock_state(false);
                        }
                        if runtime_changed {
                            parakeet = None;
                        } else if model_changed {
                            if let Some(engine) = &mut parakeet {
                                engine.unload_context();
                            }
                        }
                        tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                        tracing::info!(hotkey = %record_hotkey.label, enabled = config.hotkey_enabled, "configuration reloaded from settings");
                    }
                    Err(error) => {
                        tray.set_status(TrayStatus::Error);
                        tracing::error!(%error, "configuration reload from settings failed");
                    }
                }
            },
            default(Duration::from_millis(100)) => {
                let current_fingerprint = config_fingerprint();
                let mut loaded_config = None;
                let config_changed = if current_fingerprint != last_config_fingerprint {
                    true
                } else {
                    match AppConfig::load() {
                        Ok(disk_config) => {
                            let changed = live_config_changed(&config, &disk_config);
                            loaded_config = Some(disk_config);
                            changed
                        }
                        Err(error) => {
                            tracing::debug!(%error, "live config comparison failed");
                            false
                        }
                    }
                };
                if config_changed {
                    last_config_fingerprint = current_fingerprint;
                    match loaded_config.map(Ok).unwrap_or_else(AppConfig::load) {
                        Ok(new_config) => {
                            let runtime_changed =
                                config.parakeet_runtime_dir_path() != new_config.parakeet_runtime_dir_path();
                            let model_changed =
                                config.parakeet_model_path() != new_config.parakeet_model_path();
                            config = new_config;
                            record_hotkey = hotkey::HotkeySpec::parse(&config.record_hotkey)?;
                            hotkey::set_record_hotkey(record_hotkey.clone());
                            hotkey::set_enabled(config.hotkey_enabled);
                            if config.capslock_always_off {
                                uvox::input::set_capslock_state(false);
                            }
                            if runtime_changed {
                                parakeet = None;
                            } else if model_changed {
                                if let Some(engine) = &mut parakeet {
                                    engine.unload_context();
                                }
                            }
                            tray.set_status(if config.hotkey_enabled { TrayStatus::Ready } else { TrayStatus::Disabled });
                            tracing::info!(hotkey = %record_hotkey.label, enabled = config.hotkey_enabled, "configuration file changed; reloaded live settings");
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
                    tracing::info!("idle timeout reached; releasing native CUDA Parakeet context");
                    if let Some(engine) = &mut parakeet {
                        engine.unload_context();
                    }
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

#[cfg(windows)]
fn tooltip_bench(iterations: usize) -> Result<()> {
    let overlay = uvox::overlay::OverlayHandle::spawn()?;
    overlay.show(uvox::input::foreground_window_id());
    std::thread::sleep(Duration::from_millis(50));
    let mut values = overlay.benchmark_latency(iterations);
    overlay.hide();
    values.sort();
    if values.is_empty() {
        anyhow::bail!("tooltip benchmark produced no samples");
    }
    let micros: Vec<u128> = values.iter().map(|value| value.as_micros()).collect();
    let percentile = |pct: f32| -> u128 {
        let idx = ((micros.len() - 1) as f32 * pct).round() as usize;
        micros[idx]
    };
    println!("samples: {}", micros.len());
    println!("p50: {} us", percentile(0.50));
    println!("p95: {} us", percentile(0.95));
    println!("p99: {} us", percentile(0.99));
    println!("max: {} us", micros[micros.len() - 1]);
    Ok(())
}

#[cfg(not(windows))]
fn tooltip_bench(_iterations: usize) -> Result<()> {
    bail!("Tooltip benchmark targets Windows")
}

fn config_fingerprint() -> Option<(std::time::SystemTime, u64, u64)> {
    use std::hash::{Hash, Hasher};

    let meta = std::fs::metadata(AppConfig::config_path()).ok()?;
    let bytes = std::fs::read(AppConfig::config_path()).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((meta.modified().ok()?, meta.len(), hasher.finish()))
}

fn live_config_changed(current: &AppConfig, disk: &AppConfig) -> bool {
    current.record_hotkey != disk.record_hotkey
        || current.hotkey_enabled != disk.hotkey_enabled
        || current.capslock_always_off != disk.capslock_always_off
        || current.idle_timeout_secs != disk.idle_timeout_secs
        || current.parakeet_runtime_dir != disk.parakeet_runtime_dir
        || current.parakeet_model_path != disk.parakeet_model_path
}
