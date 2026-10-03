use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
#[cfg(target_os = "linux")]
use simple_stt::config::LinuxDeliveryChoice;
use simple_stt::config::{
    replace_file_atomic, unique_atomic_temp_path, AppConfig, AppDeliveryOverride,
    LinuxAutomationBackend, LinuxHotkeyBackend, TextDeliveryMode,
};
use simple_stt::localization::tr;
#[cfg(target_os = "linux")]
use simple_stt::localization::Locale;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP: &str = "simple-stt-linux";
static DICTATION_ACTION_LOCK: Mutex<()> = Mutex::new(());
#[cfg(target_os = "linux")]
const PORTAL_APP_ID: &str = "io.github.yosef0H4.simple_stt";

#[derive(Debug, Parser)]
#[command(name = "simple-stt-linux", about = "Rust Linux shell for Simple STT")]
struct Args {
    #[command(subcommand)]
    command: LinuxCommand,
}

#[derive(Debug, Subcommand)]
enum LinuxCommand {
    Daemon {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Toggle {
        #[arg(long, default_value_t = 90.0)]
        timeout: f64,
        #[arg(long)]
        shift_insert: bool,
    },
    Stop {
        #[arg(long, default_value_t = 90.0)]
        timeout: f64,
        #[arg(long)]
        shift_insert: bool,
    },
    RetryDelivery,
    #[command(hide = true)]
    ClipboardOwner {
        #[arg(long)]
        primary: bool,
    },
    Cancel,
    UnloadModel,
    Shutdown,
    Status,
    Launch,
    Settings,
    ConfigureShortcuts,
    CycleDelivery,
    ToggleCleanup,
    PrintShortcutCommands,
    InstallUserService,
}

#[derive(Debug, Clone)]
struct CtlResult {
    ok: bool,
    message: String,
    values: BTreeMap<String, String>,
    events: Vec<CtlEvent>,
    raw: String,
}

#[derive(Debug, Clone)]
struct CtlEvent {
    values: std::collections::BTreeMap<String, String>,
    seq: u64,
    kind: String,
    session_id: String,
    level: String,
    text: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct SessionState {
    recording: bool,
    session_id: u64,
    updated_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingSource {
    Command,
    #[cfg(any(target_os = "linux", test))]
    Tray,
    #[cfg(any(target_os = "linux", test))]
    Hotkey,
}

impl RecordingSource {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "command",
            #[cfg(any(target_os = "linux", test))]
            Self::Tray => "tray",
            #[cfg(any(target_os = "linux", test))]
            Self::Hotkey => "hotkey",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingTransition {
    Start,
    Stop,
}

fn recording_transition(recording: bool) -> RecordingTransition {
    if recording {
        RecordingTransition::Stop
    } else {
        RecordingTransition::Start
    }
}

fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    simple_stt::localization::set_language(match saved_ui_locale() {
        Locale::Ar => simple_stt::config::UiLanguage::Ar,
        Locale::En => simple_stt::config::UiLanguage::En,
    });
    match Args::parse().command {
        LinuxCommand::Daemon { config } => daemon(config),
        LinuxCommand::Toggle {
            timeout,
            shift_insert,
        } => toggle(timeout, shift_insert),
        LinuxCommand::Stop {
            timeout,
            shift_insert,
        } => stop(timeout, shift_insert),
        LinuxCommand::RetryDelivery => retry_delivery(),
        LinuxCommand::ClipboardOwner { primary } => private_clipboard_owner(primary),
        LinuxCommand::Cancel => cancel(),
        LinuxCommand::UnloadModel => unload_model(),
        LinuxCommand::Shutdown => shutdown(),
        LinuxCommand::Status => status(),
        LinuxCommand::Launch => launch(),
        LinuxCommand::Settings => settings(),
        LinuxCommand::ConfigureShortcuts => configure_shortcuts(),
        LinuxCommand::CycleDelivery => toggle_linux_delivery_mode(),
        LinuxCommand::ToggleCleanup => toggle_linux_cleanup(),
        LinuxCommand::PrintShortcutCommands => print_shortcut_commands(),
        LinuxCommand::InstallUserService => install_user_service(),
    }
}

fn daemon(config: Option<PathBuf>) -> Result<()> {
    if let Some(path) = &config {
        std::env::set_var("SIMPLE_STT_CONFIG", path);
    }
    ensure_dirs()?;
    #[cfg(target_os = "linux")]
    wait_for_graphical_environment(Duration::from_secs(60));
    let capture = find_exe("simple-stt-capture")?;
    let mut cmd = ProcessCommand::new(capture);
    cmd.arg("--token")
        .arg(token()?)
        .arg("--state-file")
        .arg(state_file());
    if let Some(path) = config.as_ref() {
        cmd.arg("--config").arg(path);
    } else {
        cmd.arg("--config").arg(AppConfig::config_path());
    }
    cmd.env("SIMPLE_STT_LINUX", "1");
    let child = Arc::new(Mutex::new(Some(
        cmd.spawn().context("starting capture service")?,
    )));
    let child_pid = child
        .lock()
        .unwrap()
        .as_ref()
        .map(std::process::Child::id)
        .unwrap_or(0);
    fs::write(pid_file(), format!("{}\n", std::process::id()))
        .context("writing linux daemon pid file")?;
    let _runtime_markers = RuntimeMarkerGuard;
    println!("[{APP}] capture service pid={child_pid}");

    #[cfg(target_os = "linux")]
    let _tray_handle = start_linux_tray();

    #[cfg(target_os = "linux")]
    start_linux_hotkeys()?;

    let shutdown_child = Arc::clone(&child);
    ctrlc::set_handler(move || {
        let _ = run_ctl(["shutdown"], Duration::from_secs(5), false);
        if let Some(mut child) = shutdown_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        cleanup_runtime_markers();
        std::process::exit(0);
    })
    .context("installing signal handler")?;

    let status = child
        .lock()
        .unwrap()
        .as_mut()
        .context("capture child missing")?
        .wait()
        .context("waiting for capture service")?;
    if status.success() {
        Ok(())
    } else {
        bail!("capture service exited with {status}")
    }
}

#[cfg(target_os = "linux")]
fn start_linux_hotkeys() -> Result<()> {
    let requested = AppConfig::load()?.general.linux_hotkey_backend;
    write_hotkey_backend_state(requested, requested, "starting", None)?;
    std::thread::spawn(move || hotkey_supervisor(requested));
    Ok(())
}

#[cfg(target_os = "linux")]
fn hotkey_supervisor(requested: LinuxHotkeyBackend) {
    let mut retry = Duration::from_secs(1);
    loop {
        refresh_graphical_environment();
        let wayland = is_wayland_session();
        let has_x11 = std::env::var_os("DISPLAY").is_some();
        let resolved = match requested {
            LinuxHotkeyBackend::Auto if wayland => LinuxHotkeyBackend::Portal,
            LinuxHotkeyBackend::Auto if has_x11 => LinuxHotkeyBackend::X11,
            LinuxHotkeyBackend::Auto => {
                let _ = write_hotkey_backend_state(
                    requested,
                    LinuxHotkeyBackend::Auto,
                    "waiting_for_desktop",
                    Some("Waiting for the graphical session environment"),
                );
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            other => other,
        };
        eprintln!(
            "[{APP}] shortcut backend={} session={} desktop={}",
            hotkey_backend_label(resolved),
            std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_owned()),
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".to_owned())
        );
        let result = match resolved {
            LinuxHotkeyBackend::Portal => futures_lite::future::block_on(portal_shortcuts_loop()),
            LinuxHotkeyBackend::X11 if wayland => {
                let message = "Native X11 shortcuts cannot observe native Wayland applications";
                let _ = write_hotkey_backend_state(
                    requested,
                    LinuxHotkeyBackend::Desktop,
                    "needs_setup",
                    Some(message),
                );
                return;
            }
            LinuxHotkeyBackend::X11 => x11_shortcuts_loop(),
            LinuxHotkeyBackend::Desktop => {
                let _ = write_hotkey_backend_state(requested, resolved, "needs_setup", None);
                return;
            }
            LinuxHotkeyBackend::Auto => unreachable!(),
        };
        let error = match result {
            Ok(()) => "shortcut listener exited unexpectedly".to_owned(),
            Err(error) => format!("{error:#}"),
        };
        eprintln!(
            "[{APP}] {} shortcut listener unavailable; retrying in {}s: {error}",
            hotkey_backend_label(resolved),
            retry.as_secs()
        );
        let _ = write_hotkey_backend_state(requested, resolved, "retrying", Some(&error));
        std::thread::sleep(retry);
        retry = (retry * 2).min(Duration::from_secs(30));
    }
}

#[cfg(target_os = "linux")]
fn hotkey_backend_label(backend: LinuxHotkeyBackend) -> &'static str {
    match backend {
        LinuxHotkeyBackend::Auto => "automatic",
        LinuxHotkeyBackend::Portal => "portal",
        LinuxHotkeyBackend::X11 => "X11",
        LinuxHotkeyBackend::Desktop => "desktop-managed",
    }
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(target_os = "linux")]
fn wait_for_graphical_environment(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        refresh_graphical_environment();
        if is_wayland_session() || std::env::var_os("DISPLAY").is_some() {
            return;
        }
        if Instant::now() >= deadline {
            eprintln!("[{APP}] graphical environment was not ready after {timeout:?}; shortcut registration will keep retrying");
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(target_os = "linux")]
fn refresh_graphical_environment() {
    let Some(output) =
        command_output(ProcessCommand::new("systemctl").args(["--user", "show-environment"]))
    else {
        return;
    };
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if matches!(
            key,
            "DISPLAY"
                | "WAYLAND_DISPLAY"
                | "XDG_SESSION_TYPE"
                | "XDG_CURRENT_DESKTOP"
                | "DBUS_SESSION_BUS_ADDRESS"
        ) && !value.is_empty()
        {
            std::env::set_var(key, value);
        }
    }
}

struct RuntimeMarkerGuard;

impl Drop for RuntimeMarkerGuard {
    fn drop(&mut self) {
        cleanup_runtime_markers();
    }
}

fn cleanup_runtime_markers() {
    for path in [
        pid_file(),
        state_file(),
        shortcut_request_file(),
        shortcut_result_file(),
    ] {
        let _ = fs::remove_file(path);
    }
    if let Ok(previous) = read_session() {
        let _ = write_session(&SessionState {
            recording: false,
            session_id: next_session_id(previous.session_id),
            updated_at: now_secs(),
        });
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxTray {
    locale: Locale,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "simple-stt".to_owned()
    }

    fn title(&self) -> String {
        "Simple STT".to_owned()
    }

    fn text_direction(&self) -> ksni::TextDirection {
        if self.locale == Locale::Ar {
            ksni::TextDirection::RightToLeft
        } else {
            ksni::TextDirection::LeftToRight
        }
    }

    fn icon_name(&self) -> String {
        "audio-input-microphone".to_owned()
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Simple STT".to_owned(),
            description: simple_stt::localization::translate(self.locale, "menu.tip", &[]),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if let Err(error) = settings() {
            eprintln!("[{APP}] tray settings action failed: {error:#}");
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        let recording = read_session().is_ok_and(|state| state.recording);
        vec![
            StandardItem {
                label: simple_stt::localization::translate(
                    self.locale,
                    if recording { "menu.stop" } else { "menu.start" },
                    &[],
                ),
                icon_name: "media-record".to_owned(),
                activate: Box::new(|_| {
                    std::thread::spawn(|| {
                        if let Err(error) = toggle_recording(RecordingSource::Tray, 90.0, false) {
                            eprintln!("[{APP}] tray recording action failed: {error:#}");
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: simple_stt::localization::translate(self.locale, "menu.settings", &[]),
                icon_name: "configure".to_owned(),
                activate: Box::new(|_| {
                    if let Err(error) = settings() {
                        eprintln!("[{APP}] tray settings action failed: {error:#}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: simple_stt::localization::translate(self.locale, "menu.unload", &[]),
                icon_name: "edit-clear".to_owned(),
                activate: Box::new(|_| {
                    if let Err(error) = unload_model() {
                        eprintln!("[{APP}] tray unload action failed: {error:#}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: simple_stt::localization::translate(self.locale, "menu.close", &[]),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|_| {
                    if let Err(error) = shutdown() {
                        eprintln!("[{APP}] tray close action failed: {error:#}");
                    }
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
fn start_linux_tray() -> Option<ksni::blocking::Handle<LinuxTray>> {
    use ksni::blocking::TrayMethods;
    let locale = saved_ui_locale();
    match (LinuxTray { locale }).assume_sni_available(true).spawn() {
        Ok(handle) => {
            let watcher = handle.clone();
            std::thread::spawn(move || {
                let mut previous = (locale, false);
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    let next = (
                        saved_ui_locale(),
                        read_session().is_ok_and(|state| state.recording),
                    );
                    if next != previous {
                        let locale = next.0;
                        watcher.update(move |tray| tray.locale = locale);
                        previous = next;
                    }
                }
            });
            Some(handle)
        }
        Err(error) => {
            eprintln!("[{APP}] system tray unavailable: {error}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn saved_ui_locale() -> Locale {
    let language = fs::read(AppConfig::config_path())
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .and_then(|value| serde_json::from_value(value["general"]["ui_language"].clone()).ok())
        .unwrap_or_default();
    simple_stt::localization::resolve(language)
}

fn toggle(timeout_s: f64, shift_insert: bool) -> Result<()> {
    toggle_recording(RecordingSource::Command, timeout_s, shift_insert)
}

fn toggle_recording(source: RecordingSource, timeout_s: f64, shift_insert: bool) -> Result<()> {
    let action = DICTATION_ACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ensure_service()?;
    let state = read_session()?;
    match recording_transition(state.recording) {
        RecordingTransition::Stop => {
            println!(
                "[{APP}] recording stop requested source={} session={}",
                source.label(),
                state.session_id
            );
            let pending = request_stop_recording(state.session_id)?;
            drop(action);
            finish_stop_recording(pending, timeout_s, shift_insert)
        }
        RecordingTransition::Start => {
            let session_id = next_session_id(state.session_id);
            let next_state = SessionState {
                recording: true,
                session_id,
                updated_at: now_secs(),
            };
            // Publish the new generation before IPC so an older helper that is
            // waiting to deliver text cannot race the replacement recording.
            write_session(&next_state)?;
            let result = match run_ctl(
                ["start-recording", "--session-id", &session_id.to_string()],
                Duration::from_secs(35),
                true,
            ) {
                Ok(result) => result,
                Err(error) => {
                    let _ = write_session(&state);
                    return Err(error);
                }
            };
            if result
                .values
                .get("recording")
                .is_some_and(|value| value == "skipped")
            {
                write_session(&SessionState {
                    recording: false,
                    ..next_state
                })?;
                return Ok(());
            }
            write_seq(read_seq()?.max(max_event_seq(&result.events)))?;
            println!(
                "[{APP}] recording started source={} session={session_id}",
                source.label()
            );
            Ok(())
        }
    }
}

fn stop(timeout_s: f64, shift_insert: bool) -> Result<()> {
    let action = DICTATION_ACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ensure_service()?;
    let state = read_session()?;
    if !state.recording {
        return Ok(());
    }
    let session_id = if state.session_id == 0 {
        next_session_id(0)
    } else {
        state.session_id
    };
    let pending = request_stop_recording(session_id)?;
    drop(action);
    finish_stop_recording(pending, timeout_s, shift_insert)
}

#[derive(Debug)]
struct PendingTranscript {
    session_id: u64,
    after: u64,
    initial: TranscriptOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptOutcome {
    Pending,
    Transcript(String),
    Terminal,
}

fn request_stop_recording(session_id: u64) -> Result<PendingTranscript> {
    let after = read_seq()?;
    let result = run_ctl(
        ["stop-recording", "--session-id", &session_id.to_string()],
        Duration::from_secs(35),
        true,
    )?;
    write_session(&SessionState {
        recording: false,
        session_id,
        updated_at: now_secs(),
    })?;
    let after = after.max(max_event_seq(&result.events));
    let initial = transcript_outcome(&result.events, session_id);
    write_seq(after)?;
    Ok(PendingTranscript {
        session_id,
        after,
        initial,
    })
}

fn finish_stop_recording(
    pending: PendingTranscript,
    timeout_s: f64,
    shift_insert: bool,
) -> Result<()> {
    let outcome = match pending.initial {
        TranscriptOutcome::Pending => {
            wait_for_transcript(pending.session_id, pending.after, timeout_s)?
        }
        outcome => outcome,
    };
    let TranscriptOutcome::Transcript(transcript) = outcome else {
        println!(
            "[{APP}] no transcript produced for session={}",
            pending.session_id
        );
        return Ok(());
    };
    if !delivery_session_is_current(&read_session()?, pending.session_id) {
        signal_delivery_complete(pending.session_id)?;
        println!(
            "[{APP}] discarded superseded transcript session={}",
            pending.session_id
        );
        return Ok(());
    }
    let delivery = (|| -> Result<(&'static str, usize)> {
        let text = transform_text(&transcript)?;
        if !delivery_session_is_current(&read_session()?, pending.session_id) {
            bail!("dictation was superseded before delivery");
        }
        remember_delivery(pending.session_id, &text)?;
        let action = deliver_text(&text, shift_insert, pending.session_id)?;
        Ok((action, text.chars().count()))
    })();
    let completion = signal_delivery_complete(pending.session_id);
    let (action, chars) = delivery?;
    completion?;
    println!("[{APP}] {action} transcript chars={chars}");
    Ok(())
}

fn delivery_session_is_current(state: &SessionState, session_id: u64) -> bool {
    !state.recording && state.session_id == session_id
}

fn signal_delivery_complete(session_id: u64) -> Result<()> {
    run_ctl(
        ["delivery-complete", "--session-id", &session_id.to_string()],
        Duration::from_secs(5),
        true,
    )?;
    Ok(())
}

fn memory_delivery_command(
    command: simple_stt::common::shell_protocol::ShellCommand,
) -> Result<simple_stt::common::shell_protocol::ShellResponse> {
    use simple_stt::common::shell_protocol::{
        ClientMessage, ServerMessage, SHELL_PROTOCOL_VERSION,
    };
    use std::io::{BufRead, BufReader};
    let service = simple_stt::capture::state::ServiceState::load(&state_file())?;
    let mut stream = std::net::TcpStream::connect(&service.address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut read = || -> Result<ServerMessage> {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    };
    serde_json::to_writer(
        &mut stream,
        &ClientMessage::Hello {
            protocol: SHELL_PROTOCOL_VERSION,
            token: token()?,
        },
    )?;
    stream.write_all(b"\n")?;
    anyhow::ensure!(
        matches!(
            read()?,
            ServerMessage::HelloAck {
                protocol: SHELL_PROTOCOL_VERSION,
                ..
            }
        ),
        "retry cache handshake failed"
    );
    serde_json::to_writer(
        &mut stream,
        &ClientMessage::Command {
            request_id: 1,
            command,
        },
    )?;
    stream.write_all(b"\n")?;
    let ServerMessage::Response {
        response,
        request_id: 1,
    } = read()?
    else {
        bail!("retry cache response failed")
    };
    anyhow::ensure!(response.ok, "{}", response.message);
    Ok(response)
}

fn remember_delivery(session_id: u64, text: &str) -> Result<()> {
    if !text.is_empty() && text.len() <= 1024 * 1024 {
        memory_delivery_command(
            simple_stt::common::shell_protocol::ShellCommand::RememberDelivery {
                session_id,
                text: text.to_owned(),
            },
        )?;
    }
    Ok(())
}

fn retry_delivery() -> Result<()> {
    let result = retry_delivery_inner();
    if result.is_err() {
        retry_notice("notice.retryFailed", true);
    }
    result
}

fn retry_notice(id: &str, warning: bool) {
    use simple_stt::common::shell_protocol::{NoticeLevel, ShellCommand};
    #[cfg(target_os = "linux")]
    let text = simple_stt::localization::translate(saved_ui_locale(), id, &[]);
    #[cfg(not(target_os = "linux"))]
    let text = tr(id);
    let _ = memory_delivery_command(ShellCommand::ShowNotice {
        level: if warning {
            NoticeLevel::Warning
        } else {
            NoticeLevel::Info
        },
        text,
    });
}

fn retry_delivery_inner() -> Result<()> {
    let action = DICTATION_ACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !AppConfig::load()?.general.enabled {
        retry_notice("notice.retryDisabled", true);
        return Ok(());
    }
    let response =
        memory_delivery_command(simple_stt::common::shell_protocol::ShellCommand::LastDelivery)?;
    let Some(text) = response.values.get("text").filter(|text| !text.is_empty()) else {
        retry_notice("notice.retryEmpty", true);
        return Ok(());
    };
    retry_notice("notice.retrying", false);
    #[cfg(target_os = "linux")]
    wait_retry_modifiers()?;
    invalidate_delivery_session()?;
    let cancelled = run_ctl(["cancel"], Duration::from_secs(5), true)?;
    anyhow::ensure!(cancelled.ok, "{}", cancelled.message);
    let session_id = read_session()?.session_id;
    drop(action);
    // The cached text is already transformed. Never add another trailing space.
    let outcome = deliver_text(text, false, session_id)?;
    retry_notice(retry_outcome_notice(outcome), outcome == "copied");
    println!("[{APP}] retry: {outcome}");
    Ok(())
}

fn retry_outcome_notice(outcome: &str) -> &'static str {
    match outcome {
        "copied" => "notice.retryCopied",
        "cancelled" => "notice.cancelled",
        _ => "notice.retrySent",
    }
}

#[cfg(target_os = "linux")]
fn wait_retry_modifiers() -> Result<()> {
    // Wayland exposes shortcut deactivation, not the global physical key state.
    // Allow its release events to settle before sending another chord.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        std::thread::sleep(Duration::from_millis(150));
        return Ok(());
    }
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let modifiers = KeyButMask::SHIFT | KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if connection.query_pointer(root)?.reply()?.mask & modifiers == KeyButMask::default() {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "release the shortcut modifiers before retrying"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn cancel() -> Result<()> {
    let _action = DICTATION_ACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ensure_service()?;
    invalidate_delivery_session()?;
    let _ = run_ctl(["cancel"], Duration::from_secs(5), false)?;
    // A portal shortcut can run in the long-lived daemon after Settings saves
    // another locale. Resolve this notice from disk rather than startup state.
    #[cfg(target_os = "linux")]
    let text = simple_stt::localization::translate(saved_ui_locale(), "notice.cancelled", &[]);
    #[cfg(not(target_os = "linux"))]
    let text = tr("notice.cancelled");
    let _ = run_ctl(
        ["notice", "--level", "warning", "--text", &text],
        Duration::from_secs(3),
        false,
    );
    println!("[{APP}] cancelled");
    Ok(())
}

fn unload_model() -> Result<()> {
    ensure_service()?;
    let _ = run_ctl(["unload-model"], Duration::from_secs(5), false)?;
    println!("[{APP}] unload requested");
    Ok(())
}

fn invalidate_delivery_session() -> Result<()> {
    let state = read_session()?;
    write_session(&SessionState {
        recording: false,
        session_id: next_session_id(state.session_id),
        updated_at: now_secs(),
    })
}

fn shutdown() -> Result<()> {
    invalidate_delivery_session()?;
    let result = run_ctl(["shutdown"], Duration::from_secs(5), false)?;
    if result.ok {
        println!("[{APP}] shutdown requested");
        Ok(())
    } else {
        bail!("{}", result.message)
    }
}

fn status() -> Result<()> {
    let result = run_ctl(["ping"], Duration::from_secs(3), false)?;
    print!("{}", result.raw);
    println!("token_file\t{}", token_file().display());
    println!("state_file\t{}", state_file().display());
    println!("session_file\t{}", session_file().display());
    if result.ok {
        Ok(())
    } else {
        bail!("{}", result.message)
    }
}

fn print_shortcut_commands() -> Result<()> {
    let exe = find_exe("simple-stt-linux").unwrap_or_else(|_| std::env::current_exe().unwrap());
    println!("Toggle dictation: {} toggle", exe.display());
    println!("Cancel dictation: {} cancel", exe.display());
    println!("Retry last dictation: {} retry-delivery", exe.display());
    println!("Switch delivery:  {} cycle-delivery", exe.display());
    println!("Toggle AI cleanup: {} toggle-cleanup", exe.display());
    println!("Unload model:     {} unload-model", exe.display());
    println!("Start program:    systemctl --user start simple-stt-linux.service");
    println!("Close program:    {} shutdown", exe.display());
    Ok(())
}

fn settings() -> Result<()> {
    ensure_dirs()?;
    let executable = find_exe("simple-stt-settings")?;
    ProcessCommand::new(executable)
        .arg("--state-file")
        .arg(state_file())
        .arg("--service-token")
        .arg(token()?)
        .spawn()
        .context("starting Simple STT Settings")?;
    Ok(())
}

fn launch() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let status = ProcessCommand::new("systemctl")
            .args(["--user", "start", "simple-stt-linux.service"])
            .status()
            .context("starting the Simple STT user service")?;
        anyhow::ensure!(status.success(), "systemd could not start Simple STT");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if run_ctl(["ping"], Duration::from_secs(1), false).is_ok() {
                return settings();
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        bail!("Simple STT started, but capture did not become ready within 10 seconds")
    }
    #[cfg(not(target_os = "linux"))]
    {
        settings()
    }
}

fn configure_shortcuts() -> Result<()> {
    let config = AppConfig::load()?;
    let wayland = std::env::var("XDG_SESSION_TYPE")
        .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let uses_portal = config.general.linux_hotkey_backend == LinuxHotkeyBackend::Portal
        || (config.general.linux_hotkey_backend == LinuxHotkeyBackend::Auto && wayland);
    if uses_portal && pid_file().exists() {
        let request = shortcut_request_file();
        let result = shortcut_result_file();
        let _ = fs::remove_file(&result);
        fs::write(&request, format!("{}\n", now_secs()))
            .context("requesting portal shortcut configuration")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if let Ok(message) = fs::read_to_string(&result) {
                let _ = fs::remove_file(&result);
                if let Some(error) = message.strip_prefix("error:") {
                    eprintln!("[{APP}] portal configuration unavailable: {}", error.trim());
                    break;
                }
                println!("[{APP}] {}", message.trim());
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    for (program, argument) in [
        ("kcmshell6", "kcm_keys"),
        ("systemsettings", "kcm_keys"),
        ("kcmshell5", "keys"),
        ("systemsettings5", "keys"),
        ("gnome-control-center", "keyboard"),
    ] {
        if command_exists(program) {
            ProcessCommand::new(program)
                .arg(argument)
                .spawn()
                .with_context(|| format!("opening {program}"))?;
            println!("[{APP}] opened desktop shortcut settings with {program}");
            return Ok(());
        }
    }
    print_shortcut_commands()?;
    bail!("No desktop shortcut settings application was detected; bind the commands printed above")
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum PortalEvent {
    Activated(String),
    Deactivated(String),
    Changed(Vec<(String, String)>),
    Closed,
    Tick,
}

#[cfg(target_os = "linux")]
async fn portal_shortcuts_loop() -> Result<()> {
    use ashpd::desktop::global_shortcuts::{
        BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, NewShortcut,
    };
    use ashpd::desktop::CreateSessionOptions;
    use futures_lite::FutureExt;
    use futures_util::{FutureExt as FuturesUtilFutureExt, StreamExt};

    ashpd::register_host_app(PORTAL_APP_ID.parse()?).await?;
    let portal = GlobalShortcuts::new().await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let shortcuts = [
        NewShortcut::new("record", "Record or stop dictation"),
        NewShortcut::new("cancel", "Cancel dictation"),
        NewShortcut::new("delivery", "Toggle text delivery mode"),
        NewShortcut::new("cleanup", "Toggle AI transcript cleanup"),
        NewShortcut::new("retry", "Retry last dictation"),
    ];
    let request = portal
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await?;
    let bound = request.response()?;
    write_shortcut_state(
        bound
            .shortcuts()
            .iter()
            .map(|item| (item.id().to_owned(), item.trigger_description().to_owned()))
            .collect(),
    )?;
    let requested = AppConfig::load()?.general.linux_hotkey_backend;
    write_hotkey_backend_state(requested, LinuxHotkeyBackend::Portal, "active", None)?;
    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;
    let mut changed = portal.receive_shortcuts_changed().await?;
    let mut closed = session.receive_closed().await?;
    loop {
        let activation = FuturesUtilFutureExt::map(activated.next(), |value| {
            value
                .map(|item| PortalEvent::Activated(item.shortcut_id().to_owned()))
                .unwrap_or(PortalEvent::Closed)
        });
        let deactivation = FuturesUtilFutureExt::map(deactivated.next(), |value| {
            value
                .map(|item| PortalEvent::Deactivated(item.shortcut_id().to_owned()))
                .unwrap_or(PortalEvent::Closed)
        });
        let shortcut_change = FuturesUtilFutureExt::map(changed.next(), |value| {
            value
                .map(|item| {
                    PortalEvent::Changed(
                        item.shortcuts()
                            .iter()
                            .map(|shortcut| {
                                (
                                    shortcut.id().to_owned(),
                                    shortcut.trigger_description().to_owned(),
                                )
                            })
                            .collect(),
                    )
                })
                .unwrap_or(PortalEvent::Closed)
        });
        let session_closed = FuturesUtilFutureExt::map(closed.next(), |_| PortalEvent::Closed);
        let tick =
            FuturesUtilFutureExt::map(async_io::Timer::after(Duration::from_millis(250)), |_| {
                PortalEvent::Tick
            });
        let event = activation
            .or(deactivation)
            .or(shortcut_change)
            .or(session_closed)
            .or(tick)
            .await;
        match event {
            PortalEvent::Activated(id) => {
                if !portal_activation_allowed(&id) {
                    continue;
                }
                eprintln!("[{APP}] portal shortcut activated id={id}");
                dispatch_shortcut_activation(id);
            }
            PortalEvent::Deactivated(id) => {
                dispatch_shortcut_deactivation(id);
            }
            PortalEvent::Changed(shortcuts) => write_shortcut_state(shortcuts)?,
            PortalEvent::Closed => {
                // Dropping the sender abandons a held retry when its portal
                // session disappears; it must not paste after reconnecting.
                retry_shortcut_gate()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .pending
                    .take();
                bail!("GlobalShortcuts portal session closed");
            }
            PortalEvent::Tick => {
                if shortcut_request_file().exists() {
                    let _ = fs::remove_file(shortcut_request_file());
                    let result = if portal.version() >= 2 {
                        portal
                            .configure_shortcuts(
                                &session,
                                None,
                                ConfigureShortcutsOptions::default(),
                            )
                            .await
                            .map(|_| "portal shortcut configuration opened".to_owned())
                            .map_err(|error| error.to_string())
                    } else {
                        Err(
                            "GlobalShortcuts portal version 1 does not support ConfigureShortcuts"
                                .to_owned(),
                        )
                    };
                    let body = match result {
                        Ok(message) => message,
                        Err(error) => format!("error: {error}"),
                    };
                    let _ = fs::write(shortcut_result_file(), body);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct X11Shortcut {
    id: &'static str,
    keycode: u8,
    modifiers: u16,
    label: String,
}

#[cfg(target_os = "linux")]
fn enable_x11_detectable_autorepeat(connection: &impl x11rb::connection::Connection) -> Result<()> {
    // Do not interpret the synthetic release/press pairs of X11 autorepeat as
    // real releases. They would otherwise deliver a held retry repeatedly.
    {
        use x11rb::protocol::xkb::{ConnectionExt, PerClientFlag, ID};
        connection.xkb_use_extension(1, 0)?.reply()?;
        let flag = PerClientFlag::DETECTABLE_AUTO_REPEAT;
        let reply = connection
            .xkb_per_client_flags(
                ID::USE_CORE_KBD.into(),
                flag,
                flag,
                0_u32.into(),
                0_u32.into(),
                0_u32.into(),
            )?
            .reply()?;
        anyhow::ensure!(
            reply.value & flag == flag,
            "X11 detectable autorepeat is unavailable"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn x11_shortcuts_loop() -> Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, EventMask, GrabMode, ModMask};
    use x11rb::protocol::Event;

    let (connection, screen_index) =
        x11rb::connect(None).context("connecting to the X11 server")?;
    let root = connection.setup().roots[screen_index].root;
    enable_x11_detectable_autorepeat(&connection)?;
    let config = AppConfig::load()?;
    let shortcuts = [
        ("record", config.general.record_hotkey.as_str()),
        ("cancel", config.general.cancel_hotkey.as_str()),
        ("delivery", config.general.toggle_delivery_hotkey.as_str()),
        ("cleanup", config.general.toggle_cleanup_hotkey.as_str()),
        ("retry", config.general.retry_delivery_hotkey.as_str()),
    ]
    .into_iter()
    .filter(|(_, chord)| !hotkey_is_none(chord))
    .map(|(id, chord)| parse_x11_shortcut(&connection, id, chord))
    .collect::<Result<Vec<_>>>()?;

    // Lock modifiers must not make an otherwise valid shortcut stop working.
    let lock_variants = [
        0_u16,
        u16::from(ModMask::LOCK),
        u16::from(ModMask::M2),
        u16::from(ModMask::LOCK | ModMask::M2),
    ];
    for shortcut in &shortcuts {
        for locks in lock_variants {
            connection
                .grab_key(
                    false,
                    root,
                    ModMask::from(shortcut.modifiers | locks),
                    shortcut.keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )?
                .check()
                .with_context(|| {
                    format!(
                        "shortcut {} conflicts with another X11 application",
                        shortcut.label
                    )
                })?;
        }
    }
    connection.change_window_attributes(
        root,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
            .event_mask(EventMask::KEY_PRESS | EventMask::KEY_RELEASE),
    )?;
    connection.flush()?;
    write_shortcut_state(
        shortcuts
            .iter()
            .map(|item| (item.id.to_owned(), item.label.clone()))
            .collect(),
    )?;
    write_hotkey_backend_state(
        config.general.linux_hotkey_backend,
        LinuxHotkeyBackend::X11,
        "active",
        None,
    )?;

    let mut pressed = BTreeMap::new();
    loop {
        match connection.wait_for_event()? {
            Event::KeyPress(event) => {
                if let Some(shortcut) = shortcuts.iter().find(|item| {
                    item.keycode == event.detail
                        && modifiers_match(item.modifiers, event.state.into())
                }) {
                    if !pressed.contains_key(&event.detail)
                        && portal_activation_allowed(shortcut.id)
                    {
                        pressed.insert(event.detail, shortcut.id);
                        dispatch_shortcut_activation(shortcut.id.to_owned());
                    }
                }
            }
            Event::KeyRelease(event) => {
                // The user may release a modifier before the trigger key.
                if let Some(id) = pressed.remove(&event.detail) {
                    dispatch_shortcut_deactivation(id.to_owned());
                }
            }
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
fn modifiers_match(expected: u16, actual: u16) -> bool {
    use x11rb::protocol::xproto::ModMask;
    let ignored = u16::from(ModMask::LOCK | ModMask::M2);
    actual & !ignored == expected
}

#[cfg(target_os = "linux")]
fn parse_x11_shortcut<C: x11rb::connection::Connection>(
    connection: &C,
    id: &'static str,
    chord: &str,
) -> Result<X11Shortcut> {
    use x11rb::protocol::xproto::{ConnectionExt, ModMask};
    let mut modifiers = 0_u16;
    let mut key = None;
    for part in chord
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.to_ascii_lowercase().as_str() {
            "shift" => modifiers |= u16::from(ModMask::SHIFT),
            "ctrl" | "control" => modifiers |= u16::from(ModMask::CONTROL),
            "alt" => modifiers |= u16::from(ModMask::M1),
            "meta" | "super" | "win" => modifiers |= u16::from(ModMask::M4),
            "capslock" | "caps_lock" => bail!(
                "Caps Lock cannot be used safely as an X11 chord modifier; use Meta, Ctrl, Alt, or Shift"
            ),
            value if key.replace(value.to_owned()).is_some() => {
                bail!("hotkey {chord:?} contains more than one non-modifier key")
            }
            _ => {}
        }
    }
    let key = key.context("hotkey must contain one non-modifier key")?;
    let keysym = x11_keysym(&key).with_context(|| format!("unsupported X11 key {key:?}"))?;
    let setup = connection.setup();
    let count = setup.max_keycode - setup.min_keycode + 1;
    let reply = connection
        .get_keyboard_mapping(setup.min_keycode, count)?
        .reply()?;
    let per = usize::from(reply.keysyms_per_keycode);
    let keycode = reply
        .keysyms
        .chunks(per)
        .position(|symbols| symbols.contains(&keysym))
        .map(|offset| setup.min_keycode + offset as u8)
        .with_context(|| format!("key {key:?} is not present in the active X11 keyboard layout"))?;
    Ok(X11Shortcut {
        id,
        keycode,
        modifiers,
        label: chord.to_owned(),
    })
}

#[cfg(target_os = "linux")]
fn x11_keysym(key: &str) -> Option<u32> {
    let lower = key.to_ascii_lowercase();
    if lower.chars().count() == 1 {
        return lower.chars().next().map(u32::from);
    }
    match lower.as_str() {
        "space" => Some(0x20),
        "tab" => Some(0xff09),
        "enter" | "return" => Some(0xff0d),
        "escape" | "esc" => Some(0xff1b),
        "backspace" => Some(0xff08),
        "insert" => Some(0xff63),
        "delete" => Some(0xffff),
        "home" => Some(0xff50),
        "end" => Some(0xff57),
        "pageup" => Some(0xff55),
        "pagedown" => Some(0xff56),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        value if value.starts_with('f') => value[1..]
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=35).contains(n))
            .map(|n| 0xffbd + n),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn write_hotkey_backend_state(
    requested: LinuxHotkeyBackend,
    active: LinuxHotkeyBackend,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let value = serde_json::json!({
        "requested": format!("{requested:?}").to_ascii_lowercase(),
        "active": format!("{active:?}").to_ascii_lowercase(),
        "status": status,
        "error": error,
    });
    fs::write(
        hotkey_backend_state_file(),
        serde_json::to_string_pretty(&value)? + "\n",
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn portal_activation_allowed(id: &str) -> bool {
    static LAST: std::sync::OnceLock<Mutex<BTreeMap<String, Instant>>> = std::sync::OnceLock::new();
    let now = Instant::now();
    let mut last = LAST
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if last
        .get(id)
        .is_some_and(|previous| now.duration_since(*previous) < Duration::from_millis(350))
    {
        return false;
    }
    last.insert(id.to_owned(), now);
    true
}

#[cfg(target_os = "linux")]
fn dispatch_shortcut_activation(id: String) {
    if id == "retry" {
        let Some(released) = retry_shortcut_gate()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .arm()
        else {
            return;
        };
        std::thread::spawn(move || {
            retry_notice("notice.retryRelease", false);
            if released.recv().is_ok() {
                if let Err(error) = retry_delivery() {
                    eprintln!("[{APP}] retry shortcut failed: {error:#}");
                }
            }
        });
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = handle_portal_activation(&id) {
            eprintln!("[{APP}] shortcut {id} failed: {error:#}");
        }
    });
}

#[cfg(target_os = "linux")]
fn dispatch_shortcut_deactivation(id: String) {
    if id == "retry" {
        retry_shortcut_gate()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release();
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = handle_portal_deactivation(&id) {
            eprintln!("[{APP}] shortcut release {id} failed: {error:#}");
        }
    });
}

#[cfg(target_os = "linux")]
fn handle_portal_activation(id: &str) -> Result<()> {
    match id {
        "record" => {
            let config = AppConfig::load()?;
            if config.general.recording_mode == simple_stt::config::RecordingMode::Hold
                && read_session()?.recording
            {
                Ok(())
            } else {
                toggle_recording(RecordingSource::Hotkey, 90.0, false)
            }
        }
        "cancel" => cancel(),
        "delivery" => toggle_linux_delivery_mode(),
        "cleanup" => toggle_linux_cleanup(),
        _ => Ok(()),
    }
}

// Arm synchronously before spawning work so a quick release cannot race the
// activation thread. Repeated Activated events do not queue more deliveries.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct RetryShortcutGate {
    pending: Option<std::sync::mpsc::Sender<()>>,
}

#[cfg(target_os = "linux")]
impl RetryShortcutGate {
    fn arm(&mut self) -> Option<std::sync::mpsc::Receiver<()>> {
        if self.pending.is_some() {
            return None;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.pending = Some(sender);
        Some(receiver)
    }

    fn release(&mut self) {
        if let Some(sender) = self.pending.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(target_os = "linux")]
fn retry_shortcut_gate() -> &'static Mutex<RetryShortcutGate> {
    static GATE: std::sync::OnceLock<Mutex<RetryShortcutGate>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| Mutex::new(RetryShortcutGate::default()))
}

#[cfg(target_os = "linux")]
fn handle_portal_deactivation(id: &str) -> Result<()> {
    if id != "record" {
        return Ok(());
    }
    if AppConfig::load().is_ok_and(|config| {
        config.general.recording_mode == simple_stt::config::RecordingMode::Hold
    }) {
        stop(90.0, false)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn toggle_linux_delivery_mode() -> Result<()> {
    let mut config = AppConfig::load()?;
    let modes = config
        .output
        .linux_delivery_cycle
        .iter()
        .copied()
        .filter(|choice| delivery_mode_supported(choice.backend, choice.mode))
        .collect::<Vec<_>>();
    if modes.is_empty() {
        bail!("No enabled delivery modes are compatible with the selected Linux automation tool");
    }
    let current = modes.iter().position(|choice| {
        choice.backend == config.output.linux_automation_backend
            && choice.mode == config.output.delivery_mode
    });
    let next = current.map_or(0, |index| (index + 1) % modes.len());
    let LinuxDeliveryChoice { backend, mode } = modes[next];
    config.output.linux_automation_backend = backend;
    config.output.delivery_mode = mode;
    if !config.output.enabled_delivery_modes.contains(&mode) {
        config.output.enabled_delivery_modes.push(mode);
    }
    config.save()?;
    let _ = run_ctl(["reload-config"], Duration::from_secs(5), false);
    simple_stt::localization::set_language(config.general.ui_language);
    let mode_id = match mode {
        TextDeliveryMode::SmartPaste => "delivery.smart_paste",
        TextDeliveryMode::Type => "delivery.type",
        TextDeliveryMode::Clipboard => "delivery.clipboard",
        TextDeliveryMode::PasteShiftInsert => "delivery.paste_shift_insert",
        TextDeliveryMode::PasteCtrlV => "delivery.paste_ctrl_v",
        TextDeliveryMode::PasteCtrlShiftV => "delivery.paste_ctrl_shift_v",
    };
    let text = simple_stt::localization::format(
        "notice.linuxDelivery",
        &[
            (
                "backend",
                &match backend {
                    LinuxAutomationBackend::Auto => tr("backend.auto"),
                    LinuxAutomationBackend::Native => tr("backend.native"),
                    _ => automation_backend_label(backend).to_owned(),
                },
            ),
            ("mode", &tr(mode_id)),
        ],
    );
    let _ = run_ctl(
        ["notice", "--level", "info", "--text", &text],
        Duration::from_secs(3),
        false,
    );
    println!("[{APP}] {text}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn toggle_linux_cleanup() -> Result<()> {
    let mut config = AppConfig::load()?;
    config.cleanup.enabled = !config.cleanup.enabled;
    let enabled = config.cleanup.enabled;
    if !enabled {
        config.cleanup.screenshot.enabled = false;
    }
    config.save()?;
    let result = run_ctl(["reload-config"], Duration::from_secs(5), false)?;
    anyhow::ensure!(result.ok, "{}", result.message);
    println!(
        "[{APP}] AI cleanup {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn toggle_linux_cleanup() -> Result<()> {
    bail!("AI cleanup shortcut toggling is only available on Linux")
}

#[cfg(target_os = "linux")]
fn hotkey_is_none(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "none" | "off" | "disabled"
    )
}

#[cfg(not(target_os = "linux"))]
fn toggle_linux_delivery_mode() -> Result<()> {
    bail!("delivery cycling is only available on Linux")
}

#[cfg(any(target_os = "linux", test))]
fn automation_backend_label(backend: LinuxAutomationBackend) -> &'static str {
    match backend {
        LinuxAutomationBackend::Auto => "Automatic",
        LinuxAutomationBackend::Native => "Native paste",
        LinuxAutomationBackend::Wtype => "wtype",
        LinuxAutomationBackend::Ydotool => "ydotool",
        LinuxAutomationBackend::Xdotool => "xdotool",
        LinuxAutomationBackend::ClipboardOnly => "wl-clipboard",
    }
}

#[cfg(test)]
fn delivery_mode_label(mode: TextDeliveryMode) -> &'static str {
    match mode {
        TextDeliveryMode::Type => "Type",
        TextDeliveryMode::SmartPaste => "Smart Paste",
        TextDeliveryMode::PasteShiftInsert => "Shift+Insert",
        TextDeliveryMode::PasteCtrlV => "Ctrl+V",
        TextDeliveryMode::PasteCtrlShiftV => "Ctrl+Shift+V",
        TextDeliveryMode::Clipboard => "Clipboard",
    }
}

#[cfg(any(target_os = "linux", test))]
fn delivery_mode_supported(backend: LinuxAutomationBackend, mode: TextDeliveryMode) -> bool {
    match backend {
        LinuxAutomationBackend::ClipboardOnly => mode == TextDeliveryMode::Clipboard,
        LinuxAutomationBackend::Native => mode != TextDeliveryMode::Type,
        _ => true,
    }
}

#[cfg(target_os = "linux")]
fn write_shortcut_state(shortcuts: Vec<(String, String)>) -> Result<()> {
    let value = shortcuts.into_iter().collect::<BTreeMap<_, _>>();
    fs::write(
        shortcut_state_file(),
        serde_json::to_string_pretty(&value)? + "\n",
    )?;
    Ok(())
}

fn install_user_service() -> Result<()> {
    let unit_dir = config_home().join("systemd").join("user");
    fs::create_dir_all(&unit_dir).context("creating systemd user dir")?;
    let exe = std::env::current_exe().context("resolving current executable")?;
    let unit = unit_dir.join("simple-stt-linux.service");
    let body = format!(
        "[Unit]\nDescription=Simple STT Linux capture daemon\nAfter=graphical-session.target xdg-desktop-portal.service\nPartOf=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} daemon\nExecStop={} shutdown\nRestart=on-failure\nRestartSec=2\nTimeoutStopSec=15\n\n[Install]\nWantedBy=graphical-session.target\n",
        exe.display(),
        exe.display()
    );
    fs::write(&unit, body).with_context(|| format!("writing {}", unit.display()))?;
    install_desktop_entries(&exe)?;
    println!("Wrote {}", unit.display());
    println!("Run:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now simple-stt-linux.service");
    Ok(())
}

fn install_desktop_entries(exe: &Path) -> Result<()> {
    let applications = data_home().join("applications");
    fs::create_dir_all(&applications)?;
    for (filename, name, command) in [
        (
            "io.github.yosef0H4.simple_stt.desktop",
            "Simple STT",
            "launch",
        ),
        (
            "simple-stt-settings.desktop",
            "Simple STT Settings",
            "settings",
        ),
        (
            "simple-stt-shortcuts.desktop",
            "Configure Simple STT Shortcuts",
            "configure-shortcuts",
        ),
    ] {
        let body = format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec={} {command}\nTerminal=false\nCategories=Settings;AudioVideo;\n",
            exe.display()
        );
        fs::write(applications.join(filename), body)?;
    }
    fs::write(
        applications.join("simple-stt-start.desktop"),
        "[Desktop Entry]\nType=Application\nName=Start Simple STT\nExec=systemctl --user start simple-stt-linux.service\nTerminal=false\nCategories=Utility;AudioVideo;\n",
    )?;
    fs::write(
        applications.join("simple-stt-stop.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Close Simple STT\nExec={} shutdown\nTerminal=false\nCategories=Utility;AudioVideo;\n", exe.display()),
    )?;
    Ok(())
}

fn ensure_service() -> Result<()> {
    let result = run_ctl(["ping"], Duration::from_secs(3), false)
        .context("capture service is not reachable. Start it with `simple-stt-linux daemon`")?;
    if result.ok {
        Ok(())
    } else {
        bail!("{}", result.message)
    }
}

fn wait_for_transcript(session_id: u64, after: u64, timeout_s: f64) -> Result<TranscriptOutcome> {
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout_s.max(1.0));
    let mut seq = after;
    while std::time::Instant::now() < deadline {
        let remaining = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .clamp(50, 250) as u64;
        let result = run_ctl(
            [
                "poll-events",
                "--after-seq",
                &seq.to_string(),
                "--wait-ms",
                &remaining.to_string(),
            ],
            Duration::from_secs(7),
            false,
        )?;
        if let Some(latest_seq) = result
            .values
            .get("latest_seq")
            .and_then(|value| value.parse::<u64>().ok())
        {
            if latest_seq < seq {
                seq = 0;
                write_seq(0)?;
                continue;
            }
        }
        seq = seq.max(max_event_seq(&result.events));
        write_seq(seq)?;
        let outcome = transcript_outcome(&result.events, session_id);
        if outcome != TranscriptOutcome::Pending {
            return Ok(outcome);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("[{APP}] transcript wait timed out for session={session_id}");
    Ok(TranscriptOutcome::Terminal)
}

fn transcript_outcome(events: &[CtlEvent], session_id: u64) -> TranscriptOutcome {
    let expected = session_id.to_string();
    for event in events {
        if event.session_id != expected {
            continue;
        }
        if event.kind == "transcript" {
            return TranscriptOutcome::Transcript(event.text.clone());
        }
        if event.kind == "notice" {
            eprintln!("[{APP}] {}: {}", event.level, event.text);
            if event
                .values
                .get("terminal")
                .is_some_and(|flag| flag == "true")
                || terminal_transcription_notice(&event.text)
            {
                return TranscriptOutcome::Terminal;
            }
        }
    }
    TranscriptOutcome::Pending
}

fn terminal_transcription_notice(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("no speech detected")
        || text.contains("recording too short")
        || text.contains("speech engine failed")
        || text.contains("speech model is missing")
        || text.contains("superseded by newer recording")
}

fn transform_text(text: &str) -> Result<String> {
    let cfg = AppConfig::load()?;
    let mut text = text.trim().to_owned();
    if cfg.output.remove_punctuation {
        text.retain(|ch| !".,!?;:".contains(ch));
    }
    if cfg.output.lowercase {
        text = text.to_lowercase();
    }
    if cfg.output.trailing_space && !text.is_empty() {
        text.push(' ');
    }
    Ok(text)
}

fn deliver_text(text: &str, force_shift_insert: bool, session_id: u64) -> Result<&'static str> {
    if text.is_empty() {
        return Ok("empty transcript skipped");
    }
    let config = AppConfig::load()?;
    let identity = focused_app_identity();
    let mode = effective_delivery_mode(
        config.output.delivery_mode,
        identity.as_deref(),
        &config.output.app_overrides,
    );
    let is_current = || read_session().is_ok_and(|s| delivery_session_is_current(&s, session_id));
    // The shell also has one-shot CLI processes: a process-local mutex cannot
    // prevent two dictations from publishing competing clipboard selections.
    let _clipboard_lock = if mode != TextDeliveryMode::Type {
        Some(acquire_clipboard_lock(&is_current)?)
    } else {
        None
    };
    if mode == TextDeliveryMode::Clipboard {
        publish_clipboard(text, false, &is_current)?.detach();
        return Ok("copied");
    }
    if mode == TextDeliveryMode::Type {
        if type_text(
            text,
            config.output.linux_automation_backend,
            typing_delay_ms(
                config.output.paced_typing_enabled,
                config.output.typing_speed_wpm,
            ),
        )? {
            return Ok("typed");
        }
        let _clipboard_lock = acquire_clipboard_lock(&is_current)?;
        publish_clipboard(text, false, &is_current)?.detach();
        eprintln!("[{APP}] typing failed; transcript is in the clipboard");
        return Ok("copied");
    }
    if config.output.preserve_clipboard
        && is_current()
        && insert_text_without_clipboard(text, &is_current)?
    {
        return Ok("inserted directly (clipboard untouched)");
    }
    paste_text(
        text,
        force_shift_insert,
        config.output.linux_automation_backend,
        mode,
        identity.as_deref(),
        &is_current,
        config.output.preserve_clipboard,
    )
}

fn insert_text_without_clipboard(text: &str, is_current: &impl Fn() -> bool) -> Result<bool> {
    use std::io::Read;
    use std::sync::atomic::{AtomicBool, Ordering};
    if !is_current() {
        bail!("dictation superseded before direct insertion");
    }
    let Ok(helper) = find_native_paste_helper() else {
        return Ok(false);
    };
    let mut child = ProcessCommand::new(helper)
        .arg("--insert-text")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .context("opening direct insertion input")?;
    let mut stdout = child
        .stdout
        .take()
        .context("opening insertion acknowledgement")?;
    let bytes = text.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let attempted = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&attempted);
    let reader = std::thread::spawn(move || {
        let mut marker = [0_u8; 8];
        if stdout.read_exact(&mut marker).is_ok() && &marker == b"attempt\n" {
            signal.store(true, Ordering::SeqCst);
        }
    });
    let start = Instant::now();
    let mut timed_out = false;
    let result = loop {
        if !is_current() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(anyhow::anyhow!(
                "dictation superseded during direct insertion"
            ));
        }
        let limit = if attempted.load(Ordering::SeqCst) {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(750)
        };
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break Ok(false);
        }
        if let Some(status) = child.try_wait()? {
            break match status.code() {
                Some(0) => Ok(true),
                Some(1) => Ok(false),
                _ => Err(anyhow::anyhow!(
                    "direct insertion could not be confirmed; use Retry last dictation if needed"
                )),
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = writer.join();
    let _ = reader.join();
    // Inspect the drained signal AFTER killing/joining, so a slow parent reader
    // cannot mistake an already attempted insertion for an eligible fallback.
    if timed_out && attempted.load(Ordering::SeqCst) {
        bail!("direct insertion timed out after submission; use Retry last dictation if needed");
    }
    result
}

#[cfg(target_os = "linux")]
fn private_clipboard_owner(primary: bool) -> Result<()> {
    simple_stt::common::private_clipboard::serve_x11(primary)
}
#[cfg(not(target_os = "linux"))]
fn private_clipboard_owner(_primary: bool) -> Result<()> {
    bail!("X11 clipboard owner is Linux-only")
}

fn acquire_clipboard_lock(is_current: &impl Fn() -> bool) -> Result<fs::File> {
    let path = AppConfig::state_dir().join("clipboard-delivery.lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if !is_current() {
            bail!("dictation superseded before clipboard delivery");
        }
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => bail!("clipboard delivery is busy: {error}"),
        }
    }
}

fn clipboard_tools() -> Result<simple_stt::common::clipboard::ClipboardTools> {
    use simple_stt::common::clipboard::{ClipboardKind, ClipboardTools};
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let (Some(publisher), Some(reader)) =
            (which_like("wl-copy", ""), which_like("wl-paste", ""))
        {
            return Ok(ClipboardTools {
                kind: ClipboardKind::Wayland,
                publisher,
                reader,
            });
        }
        bail!("Install wl-clipboard to publish and verify the Wayland clipboard");
    }
    for (name, kind) in [
        ("xclip", ClipboardKind::Xclip),
        ("xsel", ClipboardKind::Xsel),
    ] {
        if let Some(path) = which_like(name, "") {
            return Ok(ClipboardTools {
                kind,
                publisher: path.clone(),
                reader: path,
            });
        }
    }
    bail!("Install xclip or xsel to publish and verify the X11 clipboard")
}

fn publish_clipboard(
    text: &str,
    primary: bool,
    is_current: &impl Fn() -> bool,
) -> Result<simple_stt::common::clipboard::PublishedClipboard> {
    clipboard_tools()?.publish(text, primary, is_current, Duration::from_secs(3))
}

fn paste_text(
    text: &str,
    force_shift_insert: bool,
    backend: LinuxAutomationBackend,
    mode: TextDeliveryMode,
    identity: Option<&str>,
    is_current: &impl Fn() -> bool,
    preserve_clipboard: bool,
) -> Result<&'static str> {
    let key = paste_key_for_mode(mode, force_shift_insert, identity);
    let publish = |primary| {
        if preserve_clipboard {
            clipboard_tools()?.publish_private(
                text,
                primary,
                is_current,
                Duration::from_secs(3),
                &std::env::current_exe()?,
            )
        } else {
            publish_clipboard(text, primary, is_current)
        }
    };
    let clipboard = publish(false)?;
    let primary = if key == PasteKey::ShiftInsert {
        match publish(true) {
            Ok(primary) => Some(primary),
            Err(error) => {
                clipboard.detach();
                return Err(error);
            }
        }
    } else {
        None
    };
    if !is_current()
        || !clipboard.verify()?
        || primary
            .as_ref()
            .is_some_and(|p| !p.verify().unwrap_or(false))
        || !is_current()
    {
        bail!("clipboard changed or dictation superseded; paste was not sent");
    }
    // Key injection has no editor-insertion acknowledgement. Never restore an
    // older payload after a timer: the target may not have read this one yet.
    let sent = send_paste_key(key, backend);
    clipboard.detach();
    if let Some(primary) = primary {
        primary.detach();
    }
    if !sent? {
        eprintln!("[{APP}] paste shortcut failed; transcript is available for manual paste");
        return Ok("copied");
    }
    Ok("paste submitted (transcript retained in clipboard)")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteKey {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

fn effective_delivery_mode(
    configured: TextDeliveryMode,
    identity: Option<&str>,
    overrides: &[AppDeliveryOverride],
) -> TextDeliveryMode {
    identity
        .and_then(|identity| {
            overrides
                .iter()
                .find(|entry| entry.app_id.eq_ignore_ascii_case(identity))
        })
        .map_or(configured, |entry| entry.mode)
}

fn paste_key_for_mode(
    mode: TextDeliveryMode,
    force_shift_insert: bool,
    identity: Option<&str>,
) -> PasteKey {
    if force_shift_insert {
        return PasteKey::ShiftInsert;
    }
    match mode {
        TextDeliveryMode::SmartPaste => smart_paste_key(identity.is_some_and(is_terminal_identity)),
        TextDeliveryMode::PasteCtrlShiftV => PasteKey::CtrlShiftV,
        TextDeliveryMode::PasteCtrlV => PasteKey::CtrlV,
        TextDeliveryMode::PasteShiftInsert => PasteKey::ShiftInsert,
        TextDeliveryMode::Type | TextDeliveryMode::Clipboard => PasteKey::ShiftInsert,
    }
}

fn smart_paste_key(terminal: bool) -> PasteKey {
    if terminal {
        PasteKey::CtrlShiftV
    } else {
        PasteKey::ShiftInsert
    }
}

fn focused_app_identity() -> Option<String> {
    find_native_paste_helper()
        .ok()
        .and_then(|helper| {
            simple_stt::common::clipboard::output_bounded(
                ProcessCommand::new(helper).arg("--active-app"),
                Duration::from_secs(3),
            )
            .ok()
        })
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|identity| identity.trim().to_owned())
        .filter(|identity| !identity.is_empty())
}

fn is_terminal_identity(identity: &str) -> bool {
    const TERMINALS: &[&str] = &[
        "konsole",
        "gnome-terminal",
        "terminal",
        "kitty",
        "alacritty",
        "terminator",
        "xterm",
        "urxvt",
        "rxvt",
        "tilix",
        "terminology",
        "wezterm",
        "foot",
        "yakuake",
        "ghostty",
        "guake",
        "tilda",
        "hyper",
        "tabby",
        "sakura",
        "warp",
        "termius",
    ];
    let identity = identity.to_ascii_lowercase();
    TERMINALS.iter().any(|terminal| identity.contains(terminal))
}

fn send_paste_key(key: PasteKey, backend: LinuxAutomationBackend) -> Result<bool> {
    let automatic = automatic_backend(false);
    let allowed = |candidate| {
        backend == candidate || (backend == LinuxAutomationBackend::Auto && automatic == candidate)
    };
    if allowed(LinuxAutomationBackend::Native) {
        if let Ok(helper) = find_native_paste_helper() {
            if run_native_paste(&helper, key) {
                return Ok(true);
            }
        }
    }
    if allowed(LinuxAutomationBackend::Ydotool) && ydotool_ready() {
        let args = match key {
            PasteKey::ShiftInsert => vec!["key", "42:1", "110:1", "110:0", "42:0"],
            PasteKey::CtrlShiftV => vec!["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"],
            PasteKey::CtrlV => vec!["key", "29:1", "47:1", "47:0", "29:0"],
        };
        if run_paste_helper(ProcessCommand::new("ydotool").args(args)) {
            return Ok(true);
        }
    }
    if allowed(LinuxAutomationBackend::Wtype)
        && command_exists("wtype")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        let args = match key {
            PasteKey::ShiftInsert => vec!["-M", "shift", "-k", "Insert", "-m", "shift"],
            PasteKey::CtrlShiftV => vec![
                "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
            ],
            PasteKey::CtrlV => vec!["-M", "ctrl", "-k", "v", "-m", "ctrl"],
        };
        if run_paste_helper(ProcessCommand::new("wtype").args(args)) {
            return Ok(true);
        }
    }
    if allowed(LinuxAutomationBackend::Xdotool)
        && command_exists("xdotool")
        && std::env::var_os("DISPLAY").is_some()
    {
        let key = match key {
            PasteKey::ShiftInsert => "shift+Insert",
            PasteKey::CtrlShiftV => "ctrl+shift+v",
            PasteKey::CtrlV => "ctrl+v",
        };
        if run_paste_helper(ProcessCommand::new("xdotool").args(["key", "--clearmodifiers", key])) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn run_paste_helper(command: &mut ProcessCommand) -> bool {
    simple_stt::common::clipboard::output_bounded(command, Duration::from_secs(3))
        .is_ok_and(|output| output.status.success())
}

fn run_native_paste(helper: &Path, key: PasteKey) -> bool {
    let mut command = ProcessCommand::new(helper);
    let portal = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let restore_path = AppConfig::state_dir().join("native-paste-restore-token");
    if portal {
        command.arg("--portal");
        if let Ok(token) = fs::read_to_string(&restore_path) {
            let token = token.trim();
            if !token.is_empty() {
                command.arg("--restore-token").arg(token);
            }
        }
    }
    match key {
        PasteKey::ShiftInsert => {
            command.arg("--shift-insert");
        }
        PasteKey::CtrlShiftV => {
            command.arg("--terminal");
        }
        PasteKey::CtrlV => {}
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(output) =
        simple_stt::common::clipboard::output_bounded(&mut command, Duration::from_secs(12))
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    if portal {
        if let Ok(token) = String::from_utf8(output.stdout) {
            let token = token.trim();
            if !token.is_empty() {
                if let Some(parent) = restore_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = write_private_restore_token(&restore_path, token);
            }
        }
    }
    true
}

fn write_private_restore_token(path: &Path, token: &str) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(token.as_bytes())
}

fn typing_delay_ms(paced: bool, words_per_minute: u64) -> u64 {
    if paced {
        60_000 / words_per_minute.max(1) / 5
    } else {
        0
    }
}

fn type_text(text: &str, backend: LinuxAutomationBackend, delay_ms: u64) -> Result<bool> {
    let automatic = automatic_backend(true);
    let allowed = |candidate| {
        backend == candidate || (backend == LinuxAutomationBackend::Auto && automatic == candidate)
    };
    if allowed(LinuxAutomationBackend::Ydotool) && ydotool_ready() {
        let delay = delay_ms.to_string();
        if run_quiet(ProcessCommand::new("ydotool").args([
            "type",
            "--key-delay",
            &delay,
            "--",
            text,
        ])) {
            return Ok(true);
        }
    }
    if allowed(LinuxAutomationBackend::Wtype)
        && command_exists("wtype")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        let delay = delay_ms.to_string();
        if run_quiet(ProcessCommand::new("wtype").args(["-d", &delay, "--", text])) {
            return Ok(true);
        }
    }
    if allowed(LinuxAutomationBackend::Xdotool)
        && command_exists("xdotool")
        && std::env::var_os("DISPLAY").is_some()
    {
        let delay = delay_ms.to_string();
        if run_quiet(ProcessCommand::new("xdotool").args([
            "type",
            "--clearmodifiers",
            "--delay",
            &delay,
            "--",
            text,
        ])) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn automatic_backend(for_typing: bool) -> LinuxAutomationBackend {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let native = !for_typing && find_native_paste_helper().is_ok();
    let ydotool = ydotool_ready();
    let wtype = wayland && command_exists("wtype");
    let xdotool = !wayland && command_exists("xdotool");

    if !wayland && xdotool {
        return LinuxAutomationBackend::Xdotool;
    }
    if wayland && (desktop.contains("kde") || desktop.contains("plasma")) {
        if native {
            return LinuxAutomationBackend::Native;
        }
        if ydotool {
            return LinuxAutomationBackend::Ydotool;
        }
    }
    if wayland && desktop.contains("gnome") && ydotool {
        return LinuxAutomationBackend::Ydotool;
    }
    if wayland && wtype {
        return LinuxAutomationBackend::Wtype;
    }
    if ydotool {
        return LinuxAutomationBackend::Ydotool;
    }
    if native {
        return LinuxAutomationBackend::Native;
    }
    if xdotool {
        return LinuxAutomationBackend::Xdotool;
    }
    LinuxAutomationBackend::ClipboardOnly
}

fn ydotool_ready() -> bool {
    command_exists("ydotool")
        && ProcessCommand::new("pidof")
            .arg("ydotoold")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

fn find_native_paste_helper() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    for path in [
        exe.parent()
            .unwrap_or(Path::new("."))
            .join("linux-fast-paste"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/bin/linux-fast-paste"),
        PathBuf::from("resources/bin/linux-fast-paste"),
    ] {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("native paste helper not found")
}

fn run_ctl<const N: usize>(args: [&str; N], timeout: Duration, check: bool) -> Result<CtlResult> {
    let ctl = find_exe("simple-stt-ctl")?;
    let output = ProcessCommand::new(ctl)
        .arg("--state-file")
        .arg(state_file())
        .arg("--token")
        .arg(token()?)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running control command: {}", args.join(" ")))?;
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() && check {
        bail!(
            "simple-stt-ctl failed ({}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            raw
        );
    }
    let result = parse_ctl(&raw);
    if check && !result.ok {
        bail!("{}", result.message);
    }
    let _ = timeout;
    Ok(result)
}

fn parse_ctl(raw: &str) -> CtlResult {
    let mut ok = false;
    let mut message = String::new();
    let mut values = BTreeMap::new();
    let mut events = Vec::new();
    for line in raw.lines() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "status" if parts.len() > 1 => ok = parts[1] == "ok",
            "message" if parts.len() > 1 => message = unescape(parts[1]),
            "value" if parts.len() > 2 => {
                values.insert(unescape(parts[1]), unescape(parts[2]));
            }
            "event" if parts.len() >= 6 => {
                events.push(CtlEvent {
                    values: Default::default(),
                    seq: parts[1].parse().unwrap_or(0),
                    kind: unescape(parts[2]),
                    session_id: parts[3].to_owned(),
                    level: parts[4].to_owned(),
                    text: unescape(parts[5]),
                });
            }
            "event_value" if parts.len() >= 4 => {
                if let Ok(seq) = parts[1].parse::<u64>() {
                    if let Some(event) = events.iter_mut().find(|event| event.seq == seq) {
                        event.values.insert(unescape(parts[2]), unescape(parts[3]));
                    }
                }
            }
            _ => {}
        }
    }
    CtlResult {
        ok,
        message,
        values,
        events,
        raw: raw.to_owned(),
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn max_event_seq(events: &[CtlEvent]) -> u64 {
    events.iter().map(|event| event.seq).max().unwrap_or(0)
}

fn ensure_dirs() -> Result<()> {
    fs::create_dir_all(AppConfig::local_data_dir())?;
    fs::create_dir_all(AppConfig::state_dir())?;
    let config_path = AppConfig::config_path();
    let config_dir = config_path.parent().context("config path has no parent")?;
    fs::create_dir_all(config_dir)?;
    Ok(())
}

fn token() -> Result<String> {
    ensure_dirs()?;
    if token_file().exists() {
        return Ok(fs::read_to_string(token_file())?.trim().to_owned());
    }
    let value = format!(
        "linux-token-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    fs::write(token_file(), format!("{value}\n"))?;
    Ok(value)
}

fn read_session() -> Result<SessionState> {
    if !session_file().exists() {
        return Ok(SessionState {
            recording: false,
            session_id: 0,
            updated_at: 0.0,
        });
    }
    let raw = fs::read_to_string(session_file())?;
    match serde_json::from_str(&raw) {
        Ok(state) => Ok(state),
        Err(error) => {
            eprintln!("[{APP}] ignoring invalid recording-session state: {error}");
            Ok(SessionState {
                recording: false,
                session_id: 0,
                updated_at: 0.0,
            })
        }
    }
}

fn write_session(state: &SessionState) -> Result<()> {
    ensure_dirs()?;
    write_runtime_state(
        &session_file(),
        &(serde_json::to_string_pretty(state)? + "\n"),
    )
}

fn read_seq() -> Result<u64> {
    if !seq_file().exists() {
        return Ok(0);
    }
    Ok(fs::read_to_string(seq_file())?.trim().parse().unwrap_or(0))
}

fn write_seq(seq: u64) -> Result<()> {
    ensure_dirs()?;
    write_runtime_state(&seq_file(), &format!("{seq}\n"))
}

fn write_runtime_state(path: &Path, body: &str) -> Result<()> {
    let temp = unique_atomic_temp_path(path);
    let mut file = fs::File::create(&temp)?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    replace_file_atomic(&temp, path)
}

fn next_session_id(current: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    current.saturating_add(1).max(now)
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn find_exe(name: &str) -> Result<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let current_exe = std::env::current_exe().context("resolving current executable")?;
    let mut candidates = vec![
        current_exe
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{name}{suffix}")),
        PathBuf::from(format!("{name}{suffix}")),
    ];
    if let Some(env_dir) = std::env::var_os("SIMPLE_STT_BINDIR") {
        candidates.insert(0, PathBuf::from(env_dir).join(format!("{name}{suffix}")));
    }
    if let Some(hit) = std::env::var_os("PATH").and_then(|_| which_like(name, suffix)) {
        candidates.push(hit);
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "Could not find {name}. Build first with `cargo build --release` or set SIMPLE_STT_BINDIR."
    )
}

fn which_like(name: &str, suffix: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}{suffix}")))
        .find(|candidate| candidate.is_file())
}

fn command_exists(name: &str) -> bool {
    which_like(name, "").is_some()
}

#[cfg(target_os = "linux")]
fn command_output(cmd: &mut ProcessCommand) -> Option<String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                None
            }
        })
}

fn run_quiet(cmd: &mut ProcessCommand) -> bool {
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn data_dir() -> PathBuf {
    AppConfig::local_data_dir()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        })
}

fn token_file() -> PathBuf {
    data_dir().join("linux-token")
}

fn pid_file() -> PathBuf {
    data_dir().join("linux-daemon.pid")
}

fn session_file() -> PathBuf {
    data_dir().join("linux-recording-session.json")
}

fn seq_file() -> PathBuf {
    data_dir().join("linux-last-event-seq")
}

fn state_file() -> PathBuf {
    AppConfig::state_dir().join("linux-capture-state.json")
}

fn shortcut_request_file() -> PathBuf {
    data_dir().join("linux-configure-shortcuts.request")
}

fn shortcut_result_file() -> PathBuf {
    data_dir().join("linux-configure-shortcuts.result")
}

#[cfg(target_os = "linux")]
fn shortcut_state_file() -> PathBuf {
    data_dir().join("linux-shortcuts.json")
}

#[cfg(target_os = "linux")]
fn hotkey_backend_state_file() -> PathBuf {
    data_dir().join("linux-hotkey-backend.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run only on a disposable X server via scripts/test-linux-paste.py.
    #[cfg(target_os = "linux")]
    #[test]
    fn x11_delayed_paste_end_to_end() {
        if std::env::var_os("SIMPLE_STT_PASTE_X11_E2E").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("target.py");
        fs::write(
            &script,
            r#"import tkinter as tk,sys,pathlib
root=tk.Tk();root.title('Simple STT isolated paste target')
edit=tk.Text(root);edit.pack()
def paste(event):
    def consume():
        edit.insert('insert',root.clipboard_get())
        pathlib.Path(sys.argv[3]).write_text(edit.get('1.0','end-1c'))
    root.after(int(sys.argv[1]),consume)
    return 'break'
edit.bind('<Control-v>',paste)
def ready():
    edit.focus_force()
    pathlib.Path(sys.argv[2]).write_text('ready')
root.after(150,ready);root.mainloop()
"#,
        )
        .unwrap();
        for delay in [0, 800, 2500] {
            let ready = dir.path().join(format!("ready-{delay}"));
            let result = dir.path().join(format!("result-{delay}"));
            let mut target = ProcessCommand::new("python3")
                .arg(&script)
                .arg(delay.to_string())
                .arg(&ready)
                .arg(&result)
                .spawn()
                .unwrap();
            let ready_deadline = Instant::now() + Duration::from_secs(5);
            while !ready.exists() && Instant::now() < ready_deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(ready.exists(), "test editor did not become ready");
            let text = "hello world مرحبا 🙂";
            let outcome = paste_text(
                text,
                false,
                LinuxAutomationBackend::Xdotool,
                TextDeliveryMode::PasteCtrlV,
                Some("test-editor"),
                &|| true,
                false,
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while !result.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = target.kill();
            let _ = target.wait();
            assert_eq!(
                outcome.unwrap(),
                "paste submitted (transcript retained in clipboard)"
            );
            assert_eq!(
                fs::read_to_string(result).unwrap(),
                text,
                "delayed target must receive exactly one transcript"
            );
            let clipboard = simple_stt::common::clipboard::output_bounded(
                ProcessCommand::new("xclip").args(["-selection", "clipboard", "-out"]),
                Duration::from_secs(1),
            )
            .unwrap();
            assert_eq!(
                clipboard.stdout,
                text.as_bytes(),
                "transcript must remain available after delayed consumption"
            );
        }
    }

    #[test]
    fn ctl_protocol_unescapes_fields() {
        let parsed = parse_ctl(
            "status\tok\nmessage\thello\\nworld\nevent\t7\ttranscript\t42\tinfo\tمرحبا\\tworld\n",
        );
        assert!(parsed.ok);
        assert_eq!(parsed.message, "hello\nworld");
        assert_eq!(parsed.events[0].text, "مرحبا\tworld");
    }

    #[test]
    fn arabic_terminal_notice_survives_helper_transport() {
        let parsed = parse_ctl("status\tok\nevent\t7\tnotice\t42\twarning\tلم يُرصد كلام\nevent_value\t7\tterminal\ttrue\nevent_value\t7\tmessage_id\tnotice.noSpeech\n");
        assert_eq!(
            parsed.events[0]
                .values
                .get("message_id")
                .map(String::as_str),
            Some("notice.noSpeech")
        );
        assert_eq!(
            transcript_outcome(&parsed.events, 42),
            TranscriptOutcome::Terminal
        );
    }

    #[test]
    fn next_session_id_is_monotonic() {
        assert!(next_session_id(42) >= 43);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn retry_shortcut_waits_for_release_and_ignores_held_repeats() {
        use std::sync::mpsc::TryRecvError;
        let mut gate = RetryShortcutGate::default();
        gate.release(); // An unmatched release cannot retry.
        let released = gate.arm().expect("first press arms retry");
        assert_eq!(released.try_recv(), Err(TryRecvError::Empty));
        assert!(gate.arm().is_none());
        gate.release(); // Even a release before the worker starts is retained.
        gate.release();
        assert_eq!(released.try_recv(), Ok(()));
        assert_eq!(released.try_recv(), Err(TryRecvError::Disconnected));
        assert!(gate.arm().is_some(), "next deliberate press is accepted");
    }

    #[test]
    fn retry_feedback_does_not_claim_clipboard_fallback_was_pasted() {
        assert_eq!(retry_outcome_notice("copied"), "notice.retryCopied");
        assert_eq!(retry_outcome_notice("typed"), "notice.retrySent");
        assert_eq!(
            retry_outcome_notice("paste submitted (transcript retained in clipboard)"),
            "notice.retrySent"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tray_labels_follow_resolved_locale() {
        use ksni::Tray;
        let english = LinuxTray { locale: Locale::En };
        let arabic = LinuxTray { locale: Locale::Ar };
        assert!(english.tool_tip().description.contains("Right-click"));
        assert!(arabic.tool_tip().description.contains("انقر"));
        assert_eq!(english.text_direction(), ksni::TextDirection::LeftToRight);
        assert_eq!(arabic.text_direction(), ksni::TextDirection::RightToLeft);
        assert!(arabic.menu().into_iter().any(|item| match item {
            ksni::MenuItem::Standard(item) => item.label == "الإعدادات",
            _ => false,
        }));
    }

    #[test]
    fn tray_hotkey_and_command_share_toggle_state() {
        for source in [
            RecordingSource::Tray,
            RecordingSource::Hotkey,
            RecordingSource::Command,
        ] {
            assert!(!source.label().is_empty());
            assert_eq!(recording_transition(false), RecordingTransition::Start);
            assert_eq!(recording_transition(true), RecordingTransition::Stop);
        }
    }

    #[test]
    fn terminal_transcription_events_end_wait_immediately() {
        let no_speech = CtlEvent {
            values: Default::default(),
            seq: 1,
            kind: "notice".to_owned(),
            session_id: "42".to_owned(),
            level: "warning".to_owned(),
            text: "No speech detected".to_owned(),
        };
        assert_eq!(
            transcript_outcome(&[no_speech], 42),
            TranscriptOutcome::Terminal
        );
        let transcript = CtlEvent {
            values: Default::default(),
            seq: 2,
            kind: "transcript".to_owned(),
            session_id: "42".to_owned(),
            level: "info".to_owned(),
            text: "hello".to_owned(),
        };
        assert_eq!(
            transcript_outcome(&[transcript], 42),
            TranscriptOutcome::Transcript("hello".to_owned())
        );
        let superseded = CtlEvent {
            values: Default::default(),
            seq: 3,
            kind: "notice".to_owned(),
            session_id: "42".to_owned(),
            level: "info".to_owned(),
            text: "Superseded by newer recording".to_owned(),
        };
        assert_eq!(
            transcript_outcome(&[superseded], 42),
            TranscriptOutcome::Terminal
        );
    }

    #[test]
    fn only_latest_completed_session_may_deliver() {
        let completed = SessionState {
            recording: false,
            session_id: 42,
            updated_at: 1.0,
        };
        assert!(delivery_session_is_current(&completed, 42));
        assert!(!delivery_session_is_current(&completed, 41));
        let cancelled = SessionState {
            recording: false,
            session_id: next_session_id(completed.session_id),
            ..completed
        };
        assert!(!delivery_session_is_current(&cancelled, 42));
        assert!(!delivery_session_is_current(
            &SessionState {
                recording: true,
                session_id: 43,
                updated_at: 2.0,
            },
            42
        ));
    }

    #[test]
    fn empty_transcript_does_not_publish_clipboard() {
        assert_eq!(
            deliver_text("", false, 0).unwrap(),
            "empty transcript skipped"
        );
    }

    #[test]
    fn typing_speed_matches_words_per_minute() {
        assert_eq!(typing_delay_ms(true, 60), 200);
        assert_eq!(typing_delay_ms(true, 450), 26);
        assert_eq!(typing_delay_ms(false, 60), 0);
    }

    #[test]
    fn automation_backend_limits_delivery_modes() {
        assert!(delivery_mode_supported(
            LinuxAutomationBackend::Auto,
            TextDeliveryMode::Type
        ));
        assert!(!delivery_mode_supported(
            LinuxAutomationBackend::Native,
            TextDeliveryMode::Type
        ));
        assert!(delivery_mode_supported(
            LinuxAutomationBackend::Native,
            TextDeliveryMode::Clipboard
        ));
        assert!(!delivery_mode_supported(
            LinuxAutomationBackend::ClipboardOnly,
            TextDeliveryMode::PasteCtrlV
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn duplicate_portal_activation_is_debounced() {
        let id = format!("test-{}", std::process::id());
        assert!(portal_activation_allowed(&id));
        assert!(!portal_activation_allowed(&id));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_global_shortcut_end_to_end() {
        if std::env::var_os("SIMPLE_STT_X11_E2E").is_none() {
            return;
        }
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
        use x11rb::protocol::Event;

        let (connection, screen_index) = x11rb::connect(None).expect("connect to test X server");
        enable_x11_detectable_autorepeat(&connection).expect("enable detectable autorepeat");
        let root = connection.setup().roots[screen_index].root;
        let shortcut = parse_x11_shortcut(&connection, "record", "Meta+Z").unwrap();
        connection
            .grab_key(
                false,
                root,
                ModMask::from(shortcut.modifiers),
                shortcut.keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
        let status = ProcessCommand::new("xdotool")
            .args(["key", "super+z"])
            .status()
            .expect("run xdotool");
        assert!(status.success());
        loop {
            if let Event::KeyPress(event) = connection.wait_for_event().unwrap() {
                assert_eq!(event.detail, shortcut.keycode);
                assert!(modifiers_match(shortcut.modifiers, event.state.into()));
                break;
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_keysym_supports_named_and_function_keys() {
        assert_eq!(x11_keysym("Z"), Some(u32::from('z')));
        assert_eq!(x11_keysym("Escape"), Some(0xff1b));
        assert_eq!(x11_keysym("F12"), Some(0xffc9));
        assert_eq!(x11_keysym("F36"), None);
    }

    #[test]
    fn delivery_notice_names_tool_and_method() {
        assert_eq!(
            format!(
                "{} · {}",
                automation_backend_label(LinuxAutomationBackend::Wtype),
                delivery_mode_label(TextDeliveryMode::Type)
            ),
            "wtype · Type"
        );
    }

    #[test]
    fn smart_paste_uses_terminal_shortcut_only_for_terminals() {
        assert_eq!(smart_paste_key(false), PasteKey::ShiftInsert);
        assert_eq!(smart_paste_key(true), PasteKey::CtrlShiftV);
        assert!(is_terminal_identity("kitty"));
        assert!(!is_terminal_identity("Zen Browser"));
        let overrides = [AppDeliveryOverride {
            app_id: "Example Game".to_owned(),
            mode: TextDeliveryMode::Type,
        }];
        assert_eq!(
            effective_delivery_mode(
                TextDeliveryMode::SmartPaste,
                Some("example game"),
                &overrides
            ),
            TextDeliveryMode::Type
        );
    }
}
