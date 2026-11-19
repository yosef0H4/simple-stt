use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use reader_core::config::data_dir;
use reader_core::text::MAX_TEXT_BYTES;
use reader_core::wire::{read_json_frame, write_json_frame};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "readerctl",
    about = "Control Simple TTS from the desktop or a terminal"
)]
struct Args {
    #[command(subcommand)]
    command: CommandKind,
    #[arg(long, global = true)]
    json: bool,
}
#[derive(Subcommand)]
enum CommandKind {
    Ensure,
    ReadSelection,
    ReadClipboard,
    ReadRegion,
    ReadAlong,
    CopyRegion,
    Repeat,
    ListenClipboard,
    #[command(hide = true)]
    ClipboardChanged,
    ReadText {
        #[arg(long)]
        text: Option<String>,
    },
    ReadFile {
        path: PathBuf,
    },
    Pause,
    Resume,
    Stop,
    ReplaySentence,
    NextSentence,
    PreviousSentence,
    UnloadNow,
    ServerStart,
    ServerStop,
    Status,
    Shutdown,
    Settings,
    ConfigureShortcuts,
    TestSpeech,
    TestOcr {
        #[arg(long)]
        image: Option<PathBuf>,
    },
    InstallUserService,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum LocalRequest {
    ReadStart { id: String, source: String },
    ReadChunk { id: String, text: String },
    ReadFinish { id: String },
    ReadImage { path: String },
    Action { action: String },
    Status,
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
struct LocalReply {
    ok: bool,
    message: String,
    #[serde(default)]
    state: Option<serde_json::Value>,
}

fn readerd_path() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("readerctl executable has no parent")?;
    Ok(dir.join(if cfg!(windows) {
        "readerd.exe"
    } else {
        "readerd"
    }))
}
fn ensure_daemon() -> Result<()> {
    if reader_core::local_ipc::connect(&data_dir()).is_ok() {
        return Ok(());
    }
    let executable = readerd_path()?;
    let mut command = Command::new(executable);
    command.arg("daemon");
    if std::env::var_os("SIMPLE_TTS_NO_DESKTOP").is_some() {
        command.arg("--no-desktop");
    }
    let logs = reader_core::config::data_dir().join("logs");
    std::fs::create_dir_all(&logs)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = logs.join("readerd.log");
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > 4 * 1024 * 1024) {
        let _ = std::fs::rename(&path, logs.join("readerd.previous.log"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log = options.open(path)?;
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .context("starting resident Simple TTS controller")?;
    for _ in 0..100 {
        if reader_core::local_ipc::connect(&data_dir()).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            if reader_core::local_ipc::connect(&data_dir()).is_ok() {
                return Ok(());
            }
            bail!("reader controller exited with {status}; see the daemon log");
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = child.kill();
    let _ = child.wait();
    bail!("controller did not open its local IPC endpoint; see the daemon log")
}
fn send(request: &LocalRequest, start_daemon: bool) -> Result<LocalReply> {
    if start_daemon {
        ensure_daemon()?
    }
    let mut stream = reader_core::local_ipc::connect(&data_dir())
        .context("connecting to the resident controller")?;
    #[cfg(unix)]
    stream.set_read_timeout(Some(Duration::from_secs(90)))?;
    write_json_frame(&mut stream, request)?;
    let response: LocalReply = read_json_frame(&mut stream)?;
    if !response.ok {
        bail!("{}", response.message);
    }
    Ok(response)
}
fn read_text(source: &str, text: &str) -> Result<()> {
    if text.len() > MAX_TEXT_BYTES {
        bail!("reading text is limited to 1 MiB")
    }
    let id = Uuid::new_v4().to_string();
    send(
        &LocalRequest::ReadStart {
            id: id.clone(),
            source: source.to_owned(),
        },
        true,
    )?;
    // Keep control messages under 64 KiB and only split on UTF-8 boundaries.
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + 30 * 1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1
        }
        send(
            &LocalRequest::ReadChunk {
                id: id.clone(),
                text: text[start..end].to_owned(),
            },
            false,
        )?;
        start = end;
    }
    let reply = send(&LocalRequest::ReadFinish { id }, false)?;
    println!("{}", reply.message);
    Ok(())
}
fn clipboard(primary: bool) -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        let result = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            let mut args = vec!["--no-newline"];
            if primary {
                args.push("--primary")
            };
            Command::new("wl-paste")
                .args(args)
                .output()
                .context("reading the Wayland clipboard (install wl-clipboard)")?
        } else {
            let mut args = vec![
                "-selection",
                if primary { "primary" } else { "clipboard" },
                "-o",
            ];
            Command::new("xclip")
                .args(args.drain(..))
                .output()
                .context("reading the X11 clipboard (install xclip)")?
        };
        if !result.status.success() {
            bail!(if primary {
                "the desktop did not provide a selected-text clipboard value; use Read clipboard or OCR region"
            } else {
                "clipboard is empty or unavailable"
            })
        }
        let text =
            String::from_utf8(result.stdout).context("clipboard content is not UTF-8 text")?;
        if text.trim().is_empty() {
            bail!("clipboard contains no readable text")
        };
        Ok(text)
    }
    #[cfg(windows)]
    {
        let _ = primary;
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-STA",
                "-Command",
                "[Console]::Out.Write([Windows.Forms.Clipboard]::GetText())",
            ])
            .output()
            .context("reading Windows clipboard")?;
        if !output.status.success() {
            bail!("could not read Windows clipboard")
        };
        let text = String::from_utf8(output.stdout).context("clipboard text is not UTF-8")?;
        if text.trim().is_empty() {
            bail!("clipboard is empty; copy text first")
        };
        Ok(text)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = primary;
        bail!("clipboard reading is unsupported on this platform")
    }
}
fn install_user_service() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let exe = readerd_path()?;
        let dir = dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("systemd/user");
        std::fs::create_dir_all(&dir)?;
        let unit = dir.join("simple-tts.service");
        let content = format!(
            "[Unit]\nDescription=Simple TTS desktop controller\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} daemon --foreground\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            exe.display()
        );
        std::fs::write(&unit, content)?;
        let status = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .context("reloading user systemd")?;
        if !status.success() {
            bail!("systemd user manager is unavailable")
        };
        let status = Command::new("systemctl")
            .args(["--user", "enable", "--now", "simple-tts.service"])
            .status()?;
        if !status.success() {
            bail!("could not enable Simple TTS user service")
        };
        println!("Installed {}", unit.display());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        bail!(
            "install-user-service is available on Linux; use the platform launch script on Windows"
        )
    }
}
fn restart_controller() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let enabled = Command::new("systemctl")
            .args(["--user", "is-enabled", "--quiet", "simple-tts.service"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if enabled {
            let status = Command::new("systemctl")
                .args(["--user", "restart", "simple-tts.service"])
                .status()
                .context("restarting the Simple TTS user service")?;
            anyhow::ensure!(
                status.success(),
                "could not restart the Simple TTS user service"
            );
            return Ok(());
        }
    }
    let _ = send(&LocalRequest::Shutdown, true)?;
    std::thread::sleep(Duration::from_millis(400));
    ensure_daemon()
}
fn main() -> Result<()> {
    #[cfg(windows)]
    reader_core::local_ipc::prevent_stdio_handle_inheritance();
    let args = Args::parse();
    dispatch(args.command, args.json)
}

fn dispatch(command: CommandKind, json: bool) -> Result<()> {
    match command {
        CommandKind::Ensure => ensure_daemon(),
        CommandKind::ReadSelection => show_reply(send(
            &LocalRequest::Action {
                action: "read-selection".into(),
            },
            true,
        )?),
        CommandKind::ReadClipboard => read_text("clipboard", &clipboard(false)?),
        CommandKind::ListenClipboard => show_reply(send(
            &LocalRequest::Action {
                action: "toggle-clipboard-listening".into(),
            },
            true,
        )?),
        CommandKind::ClipboardChanged => {
            let _ = send(
                &LocalRequest::Action {
                    action: "clipboard-changed".into(),
                },
                false,
            )?;
            Ok(())
        }
        CommandKind::ReadRegion => read_region(),
        CommandKind::ReadAlong => show_reply(send(
            &LocalRequest::Action {
                action: "read-along".into(),
            },
            true,
        )?),
        CommandKind::CopyRegion => show_reply(send(
            &LocalRequest::Action {
                action: "copy-region".into(),
            },
            true,
        )?),
        CommandKind::ReadText { .. } | CommandKind::ReadFile { .. } => read_input(command),
        CommandKind::Pause
        | CommandKind::Resume
        | CommandKind::Stop
        | CommandKind::ReplaySentence
        | CommandKind::NextSentence
        | CommandKind::PreviousSentence
        | CommandKind::Repeat
        | CommandKind::UnloadNow
        | CommandKind::ServerStart
        | CommandKind::ServerStop => run_control(command),
        CommandKind::Status => show_status(json),
        CommandKind::Shutdown => show_reply(send(&LocalRequest::Shutdown, false)?),
        CommandKind::Settings => open_settings(),
        CommandKind::ConfigureShortcuts
        | CommandKind::TestSpeech
        | CommandKind::TestOcr { .. }
        | CommandKind::InstallUserService => run_utility(command),
    }
}

fn read_region() -> Result<()> {
    show_reply(send(
        &LocalRequest::Action {
            action: "read-region".into(),
        },
        true,
    )?)
}

fn read_input(command: CommandKind) -> Result<()> {
    match command {
        CommandKind::ReadText { text } => {
            let mut value = text.unwrap_or_default();
            if value.is_empty() {
                std::io::stdin().read_to_string(&mut value)?;
            }
            read_text("command", &value)
        }
        CommandKind::ReadFile { path } => {
            let text = std::fs::read_to_string(path).context("reading UTF-8 text file")?;
            read_text("file", &text)
        }
        _ => unreachable!("dispatch only sends text inputs here"),
    }
}

fn run_control(command: CommandKind) -> Result<()> {
    let start_daemon = !matches!(command, CommandKind::Stop);
    let action = match command {
        CommandKind::Pause => "pause",
        CommandKind::Resume => "resume",
        CommandKind::Stop => "stop",
        CommandKind::ReplaySentence => "replay",
        CommandKind::NextSentence => "next",
        CommandKind::PreviousSentence => "previous",
        CommandKind::Repeat => "repeat",
        CommandKind::UnloadNow => "unload",
        CommandKind::ServerStart => "server_start",
        CommandKind::ServerStop => "server_stop",
        _ => unreachable!("dispatch only sends controller actions here"),
    };
    show_reply(send(
        &LocalRequest::Action {
            action: action.into(),
        },
        start_daemon,
    )?)
}

fn show_reply(reply: LocalReply) -> Result<()> {
    println!("{}", reply.message);
    Ok(())
}

fn show_status(json: bool) -> Result<()> {
    let reply = send(&LocalRequest::Status, false)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&reply.state.unwrap_or_default())?
        );
    } else {
        println!("{}", reply.message);
    }
    Ok(())
}

fn open_settings() -> Result<()> {
    // Launching from the desktop entry should activate tray and hotkeys before
    // the user presses a settings action such as Test Speech.
    ensure_daemon()?;
    let executable = std::env::current_exe()?;
    let settings = executable
        .parent()
        .context("readerctl has no parent")?
        .join(if cfg!(windows) {
            "simple-stt-settings.exe"
        } else {
            "simple-stt-settings"
        });
    let status = Command::new(settings)
        .status()
        .context("opening temporary settings process")?;
    if status.success() {
        Ok(())
    } else {
        bail!("settings process failed")
    }
}

fn run_utility(command: CommandKind) -> Result<()> {
    match command {
        CommandKind::ConfigureShortcuts => {
            restart_controller()?;
            println!("Controller restarted; configured global shortcuts were rebound.");
            Ok(())
        }
        CommandKind::TestSpeech => read_text(
            "test",
            "Simple TTS is ready. This is a short local voice check.",
        ),
        CommandKind::TestOcr { image } => {
            let path = image.context("provide --image with a screenshot crop")?;
            let reply = send(
                &LocalRequest::ReadImage {
                    path: path.to_string_lossy().into_owned(),
                },
                true,
            )?;
            show_reply(reply)
        }
        CommandKind::InstallUserService => install_user_service(),
        _ => unreachable!("dispatch only sends utility commands here"),
    }
}
