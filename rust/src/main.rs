#[cfg(not(windows))]
use anyhow::bail;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use uvox::config::AppConfig;

#[derive(Parser, Debug)]
#[command(
    name = "uvox",
    version,
    about = "CUDA-only local Nemotron push-to-talk dictation"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    /// Run the Windows push-to-talk desktop manager.
    Run,
    /// Open the lightweight native Win32 settings window.
    Settings,
    /// Print current config and ask the Python worker to validate CUDA and NeMo.
    Doctor,
    /// Print the JSON config path and current config.
    ConfigShow,
    /// Reset the JSON config to defaults.
    ConfigReset,
    /// List microphone input devices.
    ListInputs,
    /// Capture a short 16 kHz mono WAV through the Rust audio path.
    RecordTest {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        #[arg(long, default_value = "recording-test.wav")]
        output: PathBuf,
    },
    /// Type literal Unicode text through the same fixed-rate sender used by live STT.
    TypeTest { text: String },
    /// Always listen to the microphone and print transcripts until Ctrl+C.
    ListenConsole {
        /// Override worker backend: nemotron or echo.
        #[arg(long)]
        backend: Option<String>,
    },
    /// Use Windows embedded Live Captions STT with CapsLock push-to-talk typing.
    RunLiveCaptions,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        CommandKind::Run => run_app(),
        CommandKind::Settings => show_settings(),
        CommandKind::Doctor => doctor(),
        CommandKind::ConfigShow => config_show(),
        CommandKind::ConfigReset => config_reset(),
        CommandKind::ListInputs => list_inputs(),
        CommandKind::RecordTest { seconds, output } => record_test(seconds, output),
        CommandKind::TypeTest { text } => type_test(&text),
        CommandKind::ListenConsole { backend } => listen_console(backend),
        CommandKind::RunLiveCaptions => run_live_captions(),
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

fn doctor() -> Result<()> {
    let config = AppConfig::load()?;
    config.validate()?;
    println!("Rust config: OK");
    println!("Config path: {}", AppConfig::config_path().display());
    let python = config.resolve_from_repo(&config.python_executable);
    anyhow::ensure!(
        python.exists(),
        "Python worker executable is missing: {}. Run scripts/setup-worker.ps1",
        python.display()
    );
    println!("Python worker: {}", python.display());
    let status = Command::new(&python)
        .current_dir(config.resolve_from_repo(&config.worker_dir))
        .arg("-m")
        .arg("uvox_worker")
        .arg("doctor")
        .arg("--check-nemo")
        .status()
        .context("launching Python worker doctor")?;
    anyhow::ensure!(
        status.success(),
        "Python worker doctor rejected this environment"
    );
    Ok(())
}

#[cfg(windows)]
fn run_app() -> Result<()> {
    uvox::app::run(AppConfig::load()?)
}

#[cfg(not(windows))]
fn run_app() -> Result<()> {
    bail!("The desktop manager targets Windows. Python worker unit tests and CUDA CLI tests remain usable separately.")
}

#[cfg(windows)]
fn show_settings() -> Result<()> {
    uvox::gui::show_settings()
}

#[cfg(not(windows))]
fn show_settings() -> Result<()> {
    bail!("The native settings GUI targets Windows")
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
    bail!("Microphone enumeration is currently implemented for Windows")
}

#[cfg(windows)]
fn record_test(seconds: u64, output: PathBuf) -> Result<()> {
    use crossbeam_channel::bounded;
    let config = AppConfig::load()?;
    let (tx, rx) = bounded(512);
    let _capture =
        uvox::audio::start_capture(&config.audio_device_contains, config.audio_gain, tx)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&output, spec)?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Ok(frame) = rx.recv_timeout(Duration::from_millis(100)) {
            for sample in frame {
                writer.write_sample(sample)?;
            }
        }
    }
    writer.finalize()?;
    println!("Wrote {}", output.display());
    Ok(())
}

#[cfg(not(windows))]
fn record_test(_seconds: u64, _output: PathBuf) -> Result<()> {
    bail!("Microphone recording is currently implemented for Windows")
}

#[cfg(windows)]
fn type_test(text: &str) -> Result<()> {
    let config = AppConfig::load()?;
    println!("Focus a normal text box. Typing begins in two seconds...");
    thread::sleep(Duration::from_secs(2));
    for chunk in text
        .chars()
        .collect::<Vec<_>>()
        .chunks(config.typing_chunk_chars.max(1))
    {
        let value: String = chunk.iter().collect();
        uvox::input::send_unicode_text(&value)?;
        thread::sleep(Duration::from_millis(config.typing_interval_ms));
    }
    Ok(())
}

#[cfg(not(windows))]
fn type_test(_text: &str) -> Result<()> {
    bail!("Text injection is implemented for Windows")
}

#[cfg(windows)]
fn listen_console(backend: Option<String>) -> Result<()> {
    use crossbeam_channel::{bounded, select, unbounded};
    use std::collections::VecDeque;
    use uvox::worker::{WorkerEvent, WorkerHandle};

    let mut config = AppConfig::load()?;
    if let Some(backend) = backend {
        config.worker_backend = backend;
    }
    config.validate()?;

    let (audio_tx, audio_rx) = bounded::<Vec<i16>>(512);
    let _capture =
        uvox::audio::start_capture(&config.audio_device_contains, config.audio_gain, audio_tx)?;

    let (worker_tx, worker_rx) = unbounded();
    let worker = WorkerHandle::spawn(&config, worker_tx).context("starting Python worker")?;

    let session_id = 1;
    let mut started = false;
    let mut buffered: VecDeque<Vec<i16>> = VecDeque::new();
    let (exit_tx, exit_rx) = bounded::<()>(1);
    ctrlc::set_handler(move || {
        let _ = exit_tx.try_send(());
    })
    .context("installing Ctrl+C handler")?;

    println!("Listening to microphone. Press Ctrl+C to stop.");
    println!("Worker backend: {}", config.worker_backend);

    loop {
        select! {
            recv(exit_rx) -> _ => break,
            recv(worker_rx) -> message => {
                match message {
                    Ok(WorkerEvent::Loading) => println!("[status] loading model"),
                    Ok(WorkerEvent::Ready) => {
                        println!("[status] ready");
                        worker.start_session(session_id)?;
                        started = true;
                        while let Some(frame) = buffered.pop_front() {
                            worker.send_audio(session_id, &frame)?;
                        }
                    }
                    Ok(WorkerEvent::Partial { session_id: event_session, text }) if event_session == session_id => {
                        println!("[partial] {text}");
                    }
                    Ok(WorkerEvent::Commit { session_id: event_session, text }) if event_session == session_id => {
                        println!("[commit]  {text}");
                    }
                    Ok(WorkerEvent::Status(status)) => println!("[status] {status}"),
                    Ok(WorkerEvent::Error(error)) => eprintln!("[error] {error}"),
                    Ok(WorkerEvent::Disconnected) => {
                        eprintln!("[error] worker disconnected");
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            recv(audio_rx) -> message => {
                if let Ok(frame) = message {
                    if started {
                        worker.send_audio(session_id, &frame)?;
                    } else {
                        if buffered.len() >= config.ring_buffer_secs * 50 {
                            buffered.pop_front();
                        }
                        buffered.push_back(frame);
                    }
                }
            }
        }
    }

    if started {
        let _ = worker.cancel_session(session_id);
    }
    worker.shutdown();
    println!("Stopped.");
    Ok(())
}

#[cfg(not(windows))]
fn listen_console(_backend: Option<String>) -> Result<()> {
    bail!("Microphone streaming is currently implemented for Windows")
}

#[cfg(windows)]
fn run_live_captions() -> Result<()> {
    use crossbeam_channel::{bounded, select, unbounded};
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use uvox::config::repo_root;
    use uvox::hotkey::{self, HotkeyEvent};
    use uvox::input::{foreground_window_id, WindowsTextSink};
    use uvox::transcript::Typist;

    let config = AppConfig::load()?;
    let script = repo_root().join("scripts").join("live-captions-stt.ps1");
    anyhow::ensure!(script.exists(), "missing {}", script.display());

    let mut child = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script)
        .arg("-Mode")
        .arg("mic")
        .arg("-Json")
        .arg("-FinalOnly")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("starting {}", script.display()))?;

    let stdout = child
        .stdout
        .take()
        .context("capturing live captions stdout")?;
    let (line_tx, line_rx) = unbounded::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    let _ = line_tx.send(line);
                }
                Err(error) => {
                    tracing::warn!(%error, "failed reading live captions stdout");
                    break;
                }
            }
        }
    });

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
    let mut active_session: Option<(u64, isize)> = None;
    let mut next_session_id = 1_u64;

    println!("Uvox Live Captions mode is running.");
    println!("Hold CapsLock to type final recognition segments. Release CapsLock to stop typing.");
    println!("Press Ctrl+C to exit.");

    loop {
        select! {
            recv(exit_rx) -> _ => {
                tracing::info!("Ctrl+C received; shutting down live captions mode");
                break;
            },
            recv(hotkey_rx) -> message => match message {
                Ok(HotkeyEvent::CapsLockDown) if active_session.is_none() => {
                    let session_id = next_session_id;
                    next_session_id += 1;
                    let target_window = foreground_window_id();
                    typist.begin_session(session_id);
                    active_session = Some((session_id, target_window));
                    tracing::info!(session_id, target_window, "CapsLock down: Live Captions typing enabled");
                }
                Ok(HotkeyEvent::CapsLockUp) => {
                    if let Some((session_id, _)) = active_session.take() {
                        typist.cancel(session_id);
                        tracing::info!(session_id, "CapsLock up: Live Captions typing disabled");
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            },
            recv(line_rx) -> message => match message {
                Ok(line) => {
                    handle_live_captions_line(&line, active_session, &typist)?;
                }
                Err(_) => {
                    tracing::warn!("Live Captions helper stdout closed");
                    break;
                }
            },
        }
    }

    if let Some((session_id, _)) = active_session {
        typist.cancel(session_id);
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

#[cfg(windows)]
fn handle_live_captions_line(
    line: &str,
    active_session: Option<(u64, isize)>,
    typist: &uvox::transcript::Typist,
) -> Result<()> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        tracing::debug!(line = trimmed, "Live Captions helper output");
        return Ok(());
    };
    let event = value
        .get("event")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    tracing::info!(event, text, "Live Captions recognition event");
    if event != "final" || text.is_empty() {
        return Ok(());
    }
    if let Some((session_id, target_window)) = active_session {
        typist.queue(session_id, target_window, format!("{text} "));
        tracing::info!(session_id, text, "queued Live Captions text");
    } else {
        tracing::debug!(
            text,
            "ignored Live Captions final because CapsLock is not held"
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_live_captions() -> Result<()> {
    bail!("Windows Live Captions mode is only implemented for Windows")
}
