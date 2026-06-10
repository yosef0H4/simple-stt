use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use simple_stt::capture::state::ServiceState;
use simple_stt::common::line_codec::{escape_field, unescape_field};
use simple_stt::common::shell_protocol::{
    ClientMessage, NoticeLevel, ServerMessage, ShellCommand, ShellResponse, SHELL_PROTOCOL_VERSION,
};
use simple_stt::config::{
    replace_file_atomic, AppConfig, CapsLockBehavior, LogLevel, TextDeliveryMode,
};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Parser)]
#[command(
    name = "simple-stt-ctl",
    about = "One-shot SimpleStt shell-to-capture helper"
)]
struct Args {
    #[arg(long)]
    state_file: Option<PathBuf>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[command(subcommand)]
    command: CommandKind,
}
#[derive(Debug, Subcommand)]
enum CommandKind {
    Ping,
    StartRecording {
        #[arg(long)]
        session_id: u64,
    },
    StopRecording {
        #[arg(long)]
        session_id: u64,
    },
    PollEvents {
        #[arg(long, default_value_t = 0)]
        after_seq: u64,
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
    },
    ReloadConfig,
    UnloadModel,
    TestModel,
    DownloadModel {
        #[arg(long)]
        filename: String,
    },
    ListInputs,
    ListModels,
    RefreshModels,
    Notice {
        #[arg(long)]
        level: NoticeArg,
        #[arg(long)]
        text: String,
    },
    Shutdown,
    ConfigShow,
    ConfigSave {
        #[arg(long)]
        input: PathBuf,
    },
    ConfigReset,
}
#[derive(Debug, Clone, ValueEnum)]
enum NoticeArg {
    Info,
    Warning,
    Error,
}

fn main() {
    let args = Args::parse();
    let output_path = args.output.clone();
    let result = run(args);
    let body = match result {
        Ok(response) => render_response(response),
        Err(error) => format!(
            "status\terror\nmessage\t{}\n",
            escape_field(&format!("{error:#}"))
        ),
    };
    if let Some(path) = output_path {
        if let Err(error) = write_atomic(&path, &body) {
            eprintln!(
                "simple-stt-ctl failed to write {}: {error:#}",
                path.display()
            );
            std::process::exit(2);
        }
    } else {
        print!("{body}");
    }
}

fn run(args: Args) -> Result<ShellResponse> {
    match &args.command {
        CommandKind::ConfigShow => return config_show(),
        CommandKind::ConfigSave { input } => return config_save(input),
        CommandKind::ConfigReset => {
            let config = AppConfig::default();
            config.save()?;
            return config_show();
        }
        _ => {}
    }
    let state_file = args
        .state_file
        .context("--state-file is required for service commands")?;
    let token = args
        .token
        .context("--token is required for service commands")?;
    let state = ServiceState::load(&state_file)?;
    anyhow::ensure!(
        state.protocol == SHELL_PROTOCOL_VERSION,
        "capture state uses protocol {}, helper expects {}",
        state.protocol,
        SHELL_PROTOCOL_VERSION
    );
    let mut stream = TcpStream::connect(&state.address)
        .with_context(|| format!("connecting to capture service at {}", state.address))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write_json_line(
        &mut stream,
        &ClientMessage::Hello {
            protocol: SHELL_PROTOCOL_VERSION,
            token,
        },
    )?;
    let mut reader = BufReader::new(stream.try_clone()?);
    match read_json_line::<ServerMessage>(&mut reader)? {
        ServerMessage::HelloAck { protocol, .. } if protocol == SHELL_PROTOCOL_VERSION => {}
        ServerMessage::Error { code, message } => anyhow::bail!("{code}: {message}"),
        other => anyhow::bail!("unexpected handshake response: {other:?}"),
    }
    match args.command {
        CommandKind::PollEvents { after_seq, wait_ms } => {
            poll_events_wait(&mut stream, &mut reader, after_seq, wait_ms)
        }
        command => request_once(&mut stream, &mut reader, translate(command)),
    }
}
fn request_once(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    command: ShellCommand,
) -> Result<ShellResponse> {
    let request_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    write_json_line(
        stream,
        &ClientMessage::Command {
            request_id,
            command,
        },
    )?;
    match read_json_line::<ServerMessage>(reader)? {
        ServerMessage::Response {
            request_id: actual,
            response,
        } if actual == request_id => Ok(response),
        ServerMessage::Error { code, message } => anyhow::bail!("{code}: {message}"),
        other => anyhow::bail!("unexpected command response: {other:?}"),
    }
}
fn poll_events_wait(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    after_seq: u64,
    wait_ms: u64,
) -> Result<ShellResponse> {
    let deadline = Instant::now() + Duration::from_millis(wait_ms.min(5_000));
    loop {
        let response = request_once(stream, reader, ShellCommand::PollEvents { after_seq })?;
        if !response.events.is_empty() || wait_ms == 0 || Instant::now() >= deadline {
            return Ok(response);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn translate(command: CommandKind) -> ShellCommand {
    match command {
        CommandKind::Ping => ShellCommand::Ping,
        CommandKind::StartRecording { session_id } => ShellCommand::StartRecording { session_id },
        CommandKind::StopRecording { session_id } => ShellCommand::StopRecording { session_id },
        CommandKind::PollEvents { after_seq, .. } => ShellCommand::PollEvents { after_seq },
        CommandKind::ReloadConfig => ShellCommand::ReloadConfig,
        CommandKind::UnloadModel => ShellCommand::UnloadModel,
        CommandKind::TestModel => ShellCommand::TestModel,
        CommandKind::DownloadModel { filename } => ShellCommand::DownloadModel { filename },
        CommandKind::ListInputs => ShellCommand::ListInputs,
        CommandKind::ListModels => ShellCommand::ListModels,
        CommandKind::RefreshModels => ShellCommand::RefreshModels,
        CommandKind::Notice { level, text } => ShellCommand::ShowNotice {
            level: match level {
                NoticeArg::Info => NoticeLevel::Info,
                NoticeArg::Warning => NoticeLevel::Warning,
                NoticeArg::Error => NoticeLevel::Error,
            },
            text,
        },
        CommandKind::Shutdown => ShellCommand::Shutdown,
        CommandKind::ConfigShow | CommandKind::ConfigSave { .. } | CommandKind::ConfigReset => {
            unreachable!("local config commands are handled before IPC")
        }
    }
}
fn config_show() -> Result<ShellResponse> {
    let config = AppConfig::load()?;
    let mut response = ShellResponse::ok("config");
    response
        .values
        .insert("schema_version".into(), config.schema_version.to_string());
    response
        .values
        .insert("hotkey_enabled".into(), config.hotkey_enabled.to_string());
    response
        .values
        .insert("record_hotkey".into(), config.record_hotkey);
    response.values.insert(
        "capslock_behavior".into(),
        match config.capslock_behavior {
            CapsLockBehavior::PreserveTap => "preserve_tap",
            CapsLockBehavior::AlwaysOff => "always_off",
        }
        .into(),
    );
    response
        .values
        .insert("audio_device_contains".into(), config.audio_device_contains);
    response
        .values
        .insert("audio_gain".into(), config.audio_gain.to_string());
    response.values.insert(
        "typing_chunk_chars".into(),
        config.typing_chunk_chars.to_string(),
    );
    response.values.insert(
        "typing_interval_ms".into(),
        config.typing_interval_ms.to_string(),
    );
    response
        .values
        .insert("trailing_space".into(), config.trailing_space.to_string());
    response.values.insert(
        "text_delivery_mode".into(),
        match config.text_delivery_mode {
            TextDeliveryMode::Type => "type",
            TextDeliveryMode::PasteCtrlV => "paste_ctrl_v",
            TextDeliveryMode::PasteCtrlShiftV => "paste_ctrl_shift_v",
        }
        .into(),
    );
    response.values.insert(
        "remove_punctuation".into(),
        config.remove_punctuation.to_string(),
    );
    response.values.insert(
        "lowercase_output".into(),
        config.lowercase_output.to_string(),
    );
    response.values.insert(
        "idle_worker_timeout_secs".into(),
        config.idle_worker_timeout_secs.to_string(),
    );
    response.values.insert(
        "worker_shutdown_grace_ms".into(),
        config.worker_shutdown_grace_ms.to_string(),
    );
    response.values.insert(
        "start_with_windows".into(),
        config.start_with_windows.to_string(),
    );
    response.values.insert(
        "log_level".into(),
        match config.log_level {
            LogLevel::Minimal => "minimal",
            LogLevel::Normal => "normal",
            LogLevel::Debug => "debug",
            LogLevel::Extreme => "extreme",
        }
        .into(),
    );
    response.values.insert(
        "diagnostic_overlay".into(),
        config.diagnostic_overlay.to_string(),
    );
    response
        .values
        .insert("log_transcripts".into(), config.log_transcripts.to_string());
    response
        .values
        .insert("parakeet_runtime_dir".into(), config.parakeet_runtime_dir);
    response.values.insert("model_dir".into(), config.model_dir);
    response.values.insert(
        "selected_model_filename".into(),
        config.selected_model_filename,
    );
    response.values.insert(
        "config_path".into(),
        AppConfig::config_path().display().to_string(),
    );
    response.values.insert(
        "shell_log_path".into(),
        AppConfig::shell_log_path().display().to_string(),
    );
    response.values.insert(
        "capture_log_path".into(),
        AppConfig::capture_log_path().display().to_string(),
    );
    response.values.insert(
        "infer_log_path".into(),
        AppConfig::infer_log_path().display().to_string(),
    );
    response.values.insert(
        "service_state_path".into(),
        AppConfig::service_state_path().display().to_string(),
    );
    Ok(response)
}
fn config_save(input: &Path) -> Result<ShellResponse> {
    let raw = fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let mut config = AppConfig::load()?;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (encoded_key, encoded_value) = line
            .split_once('\t')
            .context("config-save input must contain tab-separated key/value lines")?;
        let key = unescape_field(encoded_key);
        let key = key
            .trim_start_matches(char::from_u32(65279).unwrap())
            .to_owned();
        let value = unescape_field(encoded_value);
        match key.as_str() {
            "hotkey_enabled" => config.hotkey_enabled = parse_bool(&value)?,
            "record_hotkey" => config.record_hotkey = value.to_owned(),
            "capslock_behavior" => {
                config.capslock_behavior = match value.as_str() {
                    "preserve_tap" => CapsLockBehavior::PreserveTap,
                    "always_off" => CapsLockBehavior::AlwaysOff,
                    _ => anyhow::bail!("invalid capslock_behavior: {value}"),
                }
            }
            "audio_device_contains" => config.audio_device_contains = value.to_owned(),
            "audio_gain" => config.audio_gain = value.parse()?,
            "typing_chunk_chars" => config.typing_chunk_chars = value.parse()?,
            "typing_interval_ms" => config.typing_interval_ms = value.parse()?,
            "trailing_space" => config.trailing_space = parse_bool(&value)?,
            "text_delivery_mode" => {
                config.text_delivery_mode = match value.as_str() {
                    "type" => TextDeliveryMode::Type,
                    "paste_ctrl_v" => TextDeliveryMode::PasteCtrlV,
                    "paste_ctrl_shift_v" => TextDeliveryMode::PasteCtrlShiftV,
                    _ => anyhow::bail!("invalid text_delivery_mode: {value}"),
                }
            }
            "remove_punctuation" => config.remove_punctuation = parse_bool(&value)?,
            "lowercase_output" => config.lowercase_output = parse_bool(&value)?,
            "idle_worker_timeout_secs" => config.idle_worker_timeout_secs = value.parse()?,
            "worker_shutdown_grace_ms" => config.worker_shutdown_grace_ms = value.parse()?,
            "start_with_windows" => config.start_with_windows = parse_bool(&value)?,
            "log_level" => {
                config.log_level = match value.as_str() {
                    "minimal" => LogLevel::Minimal,
                    "normal" => LogLevel::Normal,
                    "debug" => LogLevel::Debug,
                    "extreme" => LogLevel::Extreme,
                    _ => anyhow::bail!("invalid log_level: {value}"),
                }
            }
            "diagnostic_overlay" => config.diagnostic_overlay = parse_bool(&value)?,
            "log_transcripts" => config.log_transcripts = parse_bool(&value)?,
            "parakeet_runtime_dir" => config.parakeet_runtime_dir = value.to_owned(),
            "model_dir" => config.model_dir = value.to_owned(),
            "selected_model_filename" => config.selected_model_filename = value.to_owned(),
            _ => anyhow::bail!("unknown config key: {key}"),
        }
    }
    config.save()?;
    config_show()
}
fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => anyhow::bail!("invalid boolean: {value}"),
    }
}

fn render_response(response: ShellResponse) -> String {
    let mut output = String::new();
    output.push_str(if response.ok {
        "status\tok\n"
    } else {
        "status\terror\n"
    });
    output.push_str("message\t");
    output.push_str(&escape_field(&response.message));
    output.push('\n');
    for (key, value) in response.values {
        output.push_str("value\t");
        output.push_str(&escape_field(&key));
        output.push('\t');
        output.push_str(&escape_field(&value));
        output.push('\n');
    }
    for event in response.events {
        output.push_str("event\t");
        output.push_str(&event.seq.to_string());
        output.push('\t');
        output.push_str(&escape_field(&event.kind));
        output.push('\t');
        output.push_str(
            &event
                .session_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        );
        output.push('\t');
        output.push_str(match event.level {
            NoticeLevel::Info => "info",
            NoticeLevel::Warning => "warning",
            NoticeLevel::Error => "error",
        });
        output.push('\t');
        output.push_str(&escape_field(&event.text));
        output.push('\n');
        for (key, value) in event.values {
            output.push_str("event_value\t");
            output.push_str(&event.seq.to_string());
            output.push('\t');
            output.push_str(&escape_field(&key));
            output.push('\t');
            output.push_str(&escape_field(&value));
            output.push('\n');
        }
    }
    output
}
fn read_json_line<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Result<T> {
    let mut line = String::new();
    anyhow::ensure!(
        reader.read_line(&mut line)? > 0,
        "capture service closed the connection"
    );
    anyhow::ensure!(line.len() <= 1024 * 1024, "capture response exceeded 1 MiB");
    Ok(serde_json::from_str(line.trim_end())?)
}
fn write_json_line<T: serde::Serialize>(writer: &mut impl Write, message: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, message)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
fn write_atomic(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    replace_file_atomic(&temp, path)
}
