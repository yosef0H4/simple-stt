use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use simple_stt::capture::state::ServiceState;
use simple_stt::common::line_codec::{escape_field, unescape_field};
use simple_stt::common::shell_protocol::{
    ClientMessage, NoticeLevel, ServerMessage, ShellCommand, ShellResponse, SHELL_PROTOCOL_VERSION,
};
use simple_stt::config::{
    replace_file_atomic, unique_atomic_temp_path, AppConfig, CapsLockBehavior, InferenceDevice,
    LogLevel, RecordingMode, TextDeliveryMode, UiTheme,
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
        #[arg(long)]
        target_window: Option<i64>,
    },
    StopRecording {
        #[arg(long)]
        session_id: u64,
    },
    DeliveryComplete {
        #[arg(long)]
        session_id: u64,
    },
    RememberDelivery {
        #[arg(long)]
        session_id: u64,
    },
    LastDelivery,
    Cancel,
    PollEvents {
        #[arg(long, default_value_t = 0)]
        after_seq: u64,
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
    },
    ReloadConfig,
    UnloadModel,
    TestModel {
        #[arg(long)]
        filename: Option<String>,
        #[arg(long, value_enum, default_value = "english")]
        language: simple_stt::config::SpeechLanguage,
    },
    DownloadModel {
        #[arg(long)]
        filename: String,
    },
    ListInputs,
    ListModels,
    RefreshModels,
    ListCleanupHistory,
    ClearCleanupHistory,
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
        CommandKind::RememberDelivery { session_id } => {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_string(&mut text)?;
            anyhow::ensure!(
                text.len() <= 1024 * 1024,
                "dictation exceeds retry cache limit"
            );
            request_once(
                &mut stream,
                &mut reader,
                ShellCommand::RememberDelivery { session_id, text },
            )
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
        CommandKind::StartRecording {
            session_id,
            target_window,
        } => ShellCommand::StartRecording {
            session_id,
            target_window,
        },
        CommandKind::StopRecording { session_id } => ShellCommand::StopRecording { session_id },
        CommandKind::DeliveryComplete { session_id } => {
            ShellCommand::DeliveryComplete { session_id }
        }
        CommandKind::LastDelivery => ShellCommand::LastDelivery,
        CommandKind::RememberDelivery { .. } => unreachable!("stdin command handled separately"),
        CommandKind::Cancel => ShellCommand::Cancel,
        CommandKind::PollEvents { after_seq, .. } => ShellCommand::PollEvents { after_seq },
        CommandKind::ReloadConfig => ShellCommand::ReloadConfig,
        CommandKind::UnloadModel => ShellCommand::UnloadModel,
        CommandKind::TestModel { language, filename } => {
            ShellCommand::TestModel { language, filename }
        }
        CommandKind::DownloadModel { filename } => ShellCommand::DownloadModel { filename },
        CommandKind::ListInputs => ShellCommand::ListInputs,
        CommandKind::ListModels => ShellCommand::ListModels,
        CommandKind::RefreshModels => ShellCommand::RefreshModels,
        CommandKind::ListCleanupHistory => ShellCommand::ListCleanupHistory,
        CommandKind::ClearCleanupHistory => ShellCommand::ClearCleanupHistory,
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
    response.values.insert(
        "retry_delivery_hotkey".into(),
        config.general.retry_delivery_hotkey.clone(),
    );
    response.values.insert(
        "preserve_clipboard".into(),
        config.output.preserve_clipboard.to_string(),
    );
    response
        .values
        .insert("schema_version".into(), config.schema_version.to_string());
    response
        .values
        .insert("hotkey_enabled".into(), config.general.enabled.to_string());
    response
        .values
        .insert("record_hotkey".into(), config.general.record_hotkey.clone());
    response.values.insert(
        "recording_mode".into(),
        match config.general.recording_mode {
            RecordingMode::Hold => "hold",
            RecordingMode::Toggle => "toggle",
        }
        .into(),
    );
    response.values.insert(
        "toggle_delivery_hotkey".into(),
        config.general.toggle_delivery_hotkey.clone(),
    );
    response
        .values
        .insert("cancel_hotkey".into(), config.general.cancel_hotkey.clone());
    response.values.insert(
        "toggle_cleanup_hotkey".into(),
        config.general.toggle_cleanup_hotkey.clone(),
    );
    response
        .values
        .insert("cleanup_enabled".into(), config.cleanup.enabled.to_string());
    response.values.insert(
        "capslock_behavior".into(),
        match config.general.capslock_behavior {
            CapsLockBehavior::PreserveTap => "preserve_tap",
            CapsLockBehavior::AlwaysOff => "always_off",
        }
        .into(),
    );
    response.values.insert(
        "audio_device_contains".into(),
        config.audio.preferred_device_id.clone(),
    );
    response
        .values
        .insert("audio_gain".into(), config.audio.gain.to_string());
    response.values.insert(
        "paced_typing_enabled".into(),
        config.output.paced_typing_enabled.to_string(),
    );
    response.values.insert(
        "typing_speed_wpm".into(),
        config.output.typing_speed_wpm.to_string(),
    );
    response.values.insert(
        "trailing_space".into(),
        config.output.trailing_space.to_string(),
    );
    response.values.insert(
        "text_delivery_mode".into(),
        match config.output.delivery_mode {
            TextDeliveryMode::Type => "type",
            TextDeliveryMode::SmartPaste => "smart_paste",
            TextDeliveryMode::PasteShiftInsert => "paste_shift_insert",
            TextDeliveryMode::PasteCtrlV => "paste_ctrl_v",
            TextDeliveryMode::PasteCtrlShiftV => "paste_ctrl_shift_v",
            TextDeliveryMode::Clipboard => "clipboard",
        }
        .into(),
    );
    response.values.insert(
        "enabled_delivery_modes".into(),
        config
            .output
            .enabled_delivery_modes
            .iter()
            .map(|mode| match mode {
                TextDeliveryMode::Type => "type",
                TextDeliveryMode::SmartPaste => "smart_paste",
                TextDeliveryMode::PasteShiftInsert => "paste_shift_insert",
                TextDeliveryMode::PasteCtrlV => "paste_ctrl_v",
                TextDeliveryMode::PasteCtrlShiftV => "paste_ctrl_shift_v",
                TextDeliveryMode::Clipboard => "clipboard",
            })
            .collect::<Vec<_>>()
            .join(","),
    );
    response.values.insert(
        "app_delivery_overrides".into(),
        serde_json::to_string(&config.output.app_overrides).unwrap_or_else(|_| "[]".into()),
    );
    response.values.insert(
        "remove_punctuation".into(),
        config.output.remove_punctuation.to_string(),
    );
    response.values.insert(
        "lowercase_output".into(),
        config.output.lowercase.to_string(),
    );
    response.values.insert(
        "idle_worker_timeout_secs".into(),
        config.speech.idle_worker_timeout_secs.to_string(),
    );
    response.values.insert(
        "worker_shutdown_grace_ms".into(),
        config.speech.worker_shutdown_grace_ms.to_string(),
    );
    response.values.insert(
        "start_with_windows".into(),
        config.general.start_at_login.to_string(),
    );
    response.values.insert(
        "log_level".into(),
        match config.diagnostics.log_level {
            LogLevel::Minimal => "minimal",
            LogLevel::Normal => "normal",
            LogLevel::Debug => "debug",
            LogLevel::Extreme => "extreme",
        }
        .into(),
    );
    response.values.insert(
        "diagnostic_overlay".into(),
        config.diagnostics.diagnostic_overlay.to_string(),
    );
    response.values.insert(
        "log_transcripts".into(),
        config.diagnostics.log_transcripts.to_string(),
    );
    response.values.insert(
        "inference_device".into(),
        config.speech.inference_device.as_str().into(),
    );
    response.values.insert(
        "resolved_inference_device".into(),
        config.speech.inference_device.effective().as_str().into(),
    );
    response
        .values
        .insert("ui_theme".into(), config.general.ui_theme.as_str().into());
    response.values.insert(
        "ui_language".into(),
        config.general.ui_language.as_str().into(),
    );
    response.values.insert(
        "resolved_ui_language".into(),
        simple_stt::localization::resolve(config.general.ui_language)
            .as_str()
            .into(),
    );
    response.values.insert(
        "parakeet_runtime_dir".into(),
        config.speech.runtime_dir.clone(),
    );
    response.values.insert(
        "parakeet_runtime_dir_resolved".into(),
        config.parakeet_runtime_dir_path().display().to_string(),
    );
    response
        .values
        .insert("model_dir".into(), config.speech.model_dir.clone());
    response.values.insert(
        "model_dir_resolved".into(),
        config.model_dir_path().display().to_string(),
    );
    for (key, filename) in [
        (
            "selected_model_filename",
            config.speech.single_model_filename.as_deref(),
        ),
        (
            "single_model_filename",
            config.speech.single_model_filename.as_deref(),
        ),
        (
            "english_model_filename",
            config
                .speech
                .language_models
                .get("en")
                .and_then(|name| name.as_deref()),
        ),
        (
            "arabic_model_filename",
            config
                .speech
                .language_models
                .get("ar")
                .and_then(|name| name.as_deref()),
        ),
    ] {
        response
            .values
            .insert(key.into(), filename.unwrap_or("").into());
    }
    response.values.insert(
        "selection_mode".into(),
        config.speech.selection_mode.as_str().into(),
    );
    response.values.insert(
        "language_models".into(),
        serde_json::to_string(&config.speech.language_models)?,
    );
    response.values.insert(
        "language_mode".into(),
        if config.speech.selection_mode == simple_stt::config::ModelSelectionMode::FollowKeyboard {
            "follow_keyboard"
        } else {
            "english"
        }
        .into(),
    );
    response.values.insert(
        "config_path".into(),
        AppConfig::config_path().display().to_string(),
    );
    response.values.insert(
        "runtime_root".into(),
        simple_stt::config::runtime_root().display().to_string(),
    );
    response
        .values
        .insert("instance_id".into(), simple_stt::config::app_instance_id());
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
    apply_config_text(&mut config, &raw)?;
    config.save()?;
    config_show()
}

fn apply_config_text(config: &mut AppConfig, raw: &str) -> Result<()> {
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = decode_config_line(line)?;
        apply_config_field(config, &key, &value)?;
    }
    Ok(())
}

fn decode_config_line(line: &str) -> Result<(String, String)> {
    let (encoded_key, encoded_value) = line
        .split_once('\t')
        .context("config-save input must contain tab-separated key/value lines")?;
    let key = unescape_field(encoded_key)
        .trim_start_matches(char::from_u32(65279).unwrap())
        .to_owned();
    Ok((key, unescape_field(encoded_value)))
}

fn apply_config_field(config: &mut AppConfig, key: &str, value: &str) -> Result<()> {
    if apply_bool_config(config, key, value)? || apply_string_config(config, key, value) {
        return Ok(());
    }
    if apply_numeric_config(config, key, value)? || apply_enum_config(config, key, value)? {
        return Ok(());
    }
    anyhow::bail!("unknown config key: {key}");
}

fn apply_bool_config(config: &mut AppConfig, key: &str, value: &str) -> Result<bool> {
    if key == "cleanup_enabled" {
        config.cleanup.enabled = parse_bool(value)?;
        if !config.cleanup.enabled {
            config.cleanup.screenshot.enabled = false;
        }
        return Ok(true);
    }
    let target = match key {
        "hotkey_enabled" => &mut config.general.enabled,
        "paced_typing_enabled" => &mut config.output.paced_typing_enabled,
        "preserve_clipboard" => &mut config.output.preserve_clipboard,
        "trailing_space" => &mut config.output.trailing_space,
        "remove_punctuation" => &mut config.output.remove_punctuation,
        "lowercase_output" => &mut config.output.lowercase,
        "start_with_windows" => &mut config.general.start_at_login,
        "diagnostic_overlay" => &mut config.diagnostics.diagnostic_overlay,
        "log_transcripts" => &mut config.diagnostics.log_transcripts,
        _ => return Ok(false),
    };
    *target = parse_bool(value)?;
    Ok(true)
}

fn apply_string_config(config: &mut AppConfig, key: &str, value: &str) -> bool {
    if matches!(
        key,
        "selected_model_filename"
            | "single_model_filename"
            | "english_model_filename"
            | "arabic_model_filename"
    ) {
        let filename = if value.is_empty() || value == "None" {
            None
        } else {
            Some(value.to_owned())
        };
        match key {
            "english_model_filename" => {
                config.speech.language_models.insert("en".into(), filename);
            }
            "arabic_model_filename" => {
                config.speech.language_models.insert("ar".into(), filename);
            }
            _ => config.speech.single_model_filename = filename,
        }
        return true;
    }
    let target = match key {
        "retry_delivery_hotkey" => &mut config.general.retry_delivery_hotkey,
        "record_hotkey" => &mut config.general.record_hotkey,
        "toggle_delivery_hotkey" => &mut config.general.toggle_delivery_hotkey,
        "cancel_hotkey" => &mut config.general.cancel_hotkey,
        "toggle_cleanup_hotkey" => &mut config.general.toggle_cleanup_hotkey,
        "audio_device_contains" => &mut config.audio.preferred_device_id,
        "parakeet_runtime_dir" => &mut config.speech.runtime_dir,
        "model_dir" => &mut config.speech.model_dir,
        _ => return false,
    };
    *target = value.to_owned();
    true
}

fn apply_numeric_config(config: &mut AppConfig, key: &str, value: &str) -> Result<bool> {
    match key {
        "language_models" => config.speech.language_models = serde_json::from_str(value)?,
        "audio_gain" => config.audio.gain = value.parse()?,
        "typing_speed_wpm" => config.output.typing_speed_wpm = value.parse()?,
        "idle_worker_timeout_secs" => config.speech.idle_worker_timeout_secs = value.parse()?,
        "worker_shutdown_grace_ms" => config.speech.worker_shutdown_grace_ms = value.parse()?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_enum_config(config: &mut AppConfig, key: &str, value: &str) -> Result<bool> {
    match key {
        "capslock_behavior" => config.general.capslock_behavior = parse_capslock_behavior(value)?,
        "recording_mode" => config.general.recording_mode = parse_recording_mode(value)?,
        "text_delivery_mode" => config.output.delivery_mode = parse_text_delivery_mode(value)?,
        "log_level" => config.diagnostics.log_level = parse_log_level(value)?,
        "inference_device" => config.speech.inference_device = parse_inference_device(value)?,
        "selection_mode" => {
            config.speech.selection_mode = match value {
                "single_model" => simple_stt::config::ModelSelectionMode::SingleModel,
                "follow_keyboard" => simple_stt::config::ModelSelectionMode::FollowKeyboard,
                _ => anyhow::bail!("invalid selection_mode: {value}"),
            };
        }
        "language_mode" => {
            config.speech.selection_mode = match value {
                "follow_keyboard" => simple_stt::config::ModelSelectionMode::FollowKeyboard,
                "english" | "arabic" => {
                    let id = if value == "arabic" { "ar" } else { "en" };
                    config.speech.single_model_filename =
                        config.speech.language_models.get(id).cloned().flatten();
                    simple_stt::config::ModelSelectionMode::SingleModel
                }
                _ => anyhow::bail!("invalid language_mode: {value}"),
            };
        }
        "ui_theme" => config.general.ui_theme = parse_ui_theme(value)?,
        "ui_language" => {
            config.general.ui_language = match value {
                "auto" => simple_stt::config::UiLanguage::Auto,
                "en" => simple_stt::config::UiLanguage::En,
                "ar" => simple_stt::config::UiLanguage::Ar,
                _ => anyhow::bail!("invalid ui_language: {value}"),
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_recording_mode(value: &str) -> Result<RecordingMode> {
    match value {
        "hold" => Ok(RecordingMode::Hold),
        "toggle" => Ok(RecordingMode::Toggle),
        _ => anyhow::bail!("invalid recording_mode: {value}"),
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => anyhow::bail!("invalid boolean: {value}"),
    }
}

fn parse_capslock_behavior(value: &str) -> Result<CapsLockBehavior> {
    match value {
        "preserve_tap" => Ok(CapsLockBehavior::PreserveTap),
        "always_off" => Ok(CapsLockBehavior::AlwaysOff),
        _ => anyhow::bail!("invalid capslock_behavior: {value}"),
    }
}

fn parse_text_delivery_mode(value: &str) -> Result<TextDeliveryMode> {
    match value {
        "type" => Ok(TextDeliveryMode::Type),
        "smart_paste" => Ok(TextDeliveryMode::SmartPaste),
        "paste_shift_insert" => Ok(TextDeliveryMode::PasteShiftInsert),
        "paste_ctrl_v" => Ok(TextDeliveryMode::PasteCtrlV),
        "paste_ctrl_shift_v" => Ok(TextDeliveryMode::PasteCtrlShiftV),
        "clipboard" => Ok(TextDeliveryMode::Clipboard),
        _ => anyhow::bail!("invalid text_delivery_mode: {value}"),
    }
}

fn parse_log_level(value: &str) -> Result<LogLevel> {
    match value {
        "minimal" => Ok(LogLevel::Minimal),
        "normal" => Ok(LogLevel::Normal),
        "debug" => Ok(LogLevel::Debug),
        "extreme" => Ok(LogLevel::Extreme),
        _ => anyhow::bail!("invalid log_level: {value}"),
    }
}

fn parse_inference_device(value: &str) -> Result<InferenceDevice> {
    match value {
        "cpu" => Ok(InferenceDevice::Cpu),
        "gpu" | "nvidia_gpu" => Ok(InferenceDevice::Gpu),
        "auto" => Ok(InferenceDevice::Auto),
        _ => anyhow::bail!("invalid inference_device: {value}"),
    }
}

fn parse_ui_theme(value: &str) -> Result<UiTheme> {
    match value {
        "light" => Ok(UiTheme::Light),
        "dark" => Ok(UiTheme::Dark),
        "auto" => Ok(UiTheme::Auto),
        _ => anyhow::bail!("invalid ui_theme: {value}"),
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
    let temp = unique_atomic_temp_path(path);
    let mut file = fs::File::create(&temp)?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    replace_file_atomic(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_config_line_strips_initial_bom() {
        let (key, value) = decode_config_line("\u{feff}hotkey_enabled\tfalse").unwrap();
        assert_eq!(key, "hotkey_enabled");
        assert_eq!(value, "false");
    }

    #[test]
    fn apply_config_text_updates_supported_field_types() {
        let mut config = AppConfig::default();
        apply_config_text(
            &mut config,
            "hotkey_enabled\t0\nrecord_hotkey\tCapsLock+Q\ncancel_hotkey\tCapsLock+A\naudio_gain\t2.5\ntext_delivery_mode\tpaste_ctrl_shift_v\ninference_device\tnvidia_gpu\n",
        )
        .unwrap();

        assert!(!config.general.enabled);
        assert_eq!(config.general.record_hotkey, "CapsLock+Q");
        assert_eq!(config.general.cancel_hotkey, "CapsLock+A");
        assert_eq!(config.audio.gain, 2.5);
        assert_eq!(
            config.output.delivery_mode,
            TextDeliveryMode::PasteCtrlShiftV
        );
        assert_eq!(config.speech.inference_device, InferenceDevice::Gpu);
    }

    #[test]
    fn shell_save_preserves_single_choice_and_nullable_language_map() {
        let mut config = AppConfig::default();
        apply_config_text(&mut config, "single_model_filename\tmissing.gguf\nlanguage_models\t{\"en\":null,\"fr\":\"arabic.gguf\"}\nselection_mode\tfollow_keyboard\n").unwrap();
        assert_eq!(
            config.speech.single_model_filename.as_deref(),
            Some("missing.gguf")
        );
        assert_eq!(config.speech.language_models["en"], None);
        assert_eq!(
            config.speech.language_models["fr"].as_deref(),
            Some("arabic.gguf")
        );
        apply_config_text(&mut config, "selection_mode\tsingle_model\n").unwrap();
        assert_eq!(
            config.speech.single_model_filename.as_deref(),
            Some("missing.gguf")
        );
        assert_eq!(config.speech.language_models["en"], None);
        apply_config_text(&mut config, "language_models\t{\"fr\":\"../bad.gguf\"}\n").unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn apply_config_text_rejects_unknown_key() {
        let mut config = AppConfig::default();
        let error = apply_config_text(&mut config, "stale_setting\ttrue").unwrap_err();
        assert!(format!("{error:#}").contains("unknown config key: stale_setting"));
    }

    #[test]
    fn apply_config_text_rejects_invalid_enum_value() {
        let mut config = AppConfig::default();
        let error = apply_config_text(&mut config, "ui_theme\tneon").unwrap_err();
        assert!(format!("{error:#}").contains("invalid ui_theme: neon"));
    }
}
