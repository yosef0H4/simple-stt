#[cfg(not(windows))]
use anyhow::bail;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        CommandKind::Run => run_app(),
        CommandKind::Settings => show_settings(),
        CommandKind::ConfigShow => config_show(),
        CommandKind::ConfigReset => config_reset(),
        CommandKind::ListInputs => list_inputs(),
        CommandKind::TranscribeFile { audio } => transcribe_file(audio),
    }
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
    use uvox::parakeet_native::ParakeetNative;
    use uvox::transcript::Typist;

    let config = AppConfig::load()?;
    config.validate()?;
    config.validate_parakeet_files()?;

    let (audio_tx, audio_rx) = bounded::<Vec<i16>>(4096);
    let _capture =
        uvox::audio::start_capture(&config.audio_device_contains, config.audio_gain, audio_tx)?;
    let (hotkey_tx, hotkey_rx) = unbounded();
    let _hook = hotkey::spawn_capslock_hook(hotkey_tx)?;
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

    println!("Uvox is running.");
    println!("Hold CapsLock to record. Release CapsLock to transcribe with native CUDA Parakeet and type.");
    println!("Press Ctrl+C to exit.");

    loop {
        select! {
            recv(exit_rx) -> _ => break,
            recv(hotkey_rx) -> message => match message {
                Ok(HotkeyEvent::CapsLockDown) if active.is_none() => {
                    let session_id = next_session_id;
                    next_session_id += 1;
                    let target_window = foreground_window_id();
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
                    if let Some(recording) = active.take() {
                        let samples = recording.samples.len();
                        tracing::info!(session_id = recording.session_id, samples, "CapsLock up: transcribing");
                        if samples < 1_600 {
                            tracing::warn!(session_id = recording.session_id, samples, "ignored very short recording");
                            typist.cancel(recording.session_id);
                            continue;
                        }
                        let Some(engine) = &parakeet else {
                            tracing::error!(session_id = recording.session_id, "Parakeet runtime was not loaded");
                            typist.cancel(recording.session_id);
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
                            }
                        }
                        last_activity = Instant::now();
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
            default(Duration::from_millis(100)) => {
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
