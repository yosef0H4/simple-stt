use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use simple_stt::config::AppConfig;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP: &str = "simple-stt-linux";

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
    Cancel,
    UnloadModel,
    Shutdown,
    Status,
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
    seq: u64,
    kind: String,
    session_id: String,
    level: String,
    text: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SessionState {
    recording: bool,
    session_id: u64,
    updated_at: f64,
}

fn main() -> Result<()> {
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
        LinuxCommand::Cancel => cancel(),
        LinuxCommand::UnloadModel => unload_model(),
        LinuxCommand::Shutdown => shutdown(),
        LinuxCommand::Status => status(),
        LinuxCommand::PrintShortcutCommands => print_shortcut_commands(),
        LinuxCommand::InstallUserService => install_user_service(),
    }
}

fn daemon(config: Option<PathBuf>) -> Result<()> {
    ensure_dirs()?;
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
    println!("[{APP}] capture service pid={child_pid}");

    let shutdown_child = Arc::clone(&child);
    ctrlc::set_handler(move || {
        let _ = run_ctl(["shutdown"], Duration::from_secs(5), false);
        if let Some(mut child) = shutdown_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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

fn toggle(timeout_s: f64, shift_insert: bool) -> Result<()> {
    ensure_service()?;
    let state = read_session()?;
    if state.recording {
        stop_recording(state.session_id, timeout_s, shift_insert)
    } else {
        let session_id = next_session_id(state.session_id);
        let result = run_ctl(
            ["start-recording", "--session-id", &session_id.to_string()],
            Duration::from_secs(35),
            true,
        )?;
        write_session(&SessionState {
            recording: true,
            session_id,
            updated_at: now_secs(),
        })?;
        write_seq(read_seq()?.max(max_event_seq(&result.events)))?;
        println!("[{APP}] recording started session={session_id}");
        Ok(())
    }
}

fn stop(timeout_s: f64, shift_insert: bool) -> Result<()> {
    ensure_service()?;
    let state = read_session()?;
    let session_id = if state.session_id == 0 {
        next_session_id(0)
    } else {
        state.session_id
    };
    stop_recording(session_id, timeout_s, shift_insert)
}

fn stop_recording(session_id: u64, timeout_s: f64, shift_insert: bool) -> Result<()> {
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
    let transcript = wait_for_transcript(session_id, after, timeout_s)?;
    if transcript.is_empty() {
        println!("[{APP}] no transcript produced for session={session_id}");
        return Ok(());
    }
    let text = transform_text(&transcript)?;
    if paste_text(&text, shift_insert)? {
        println!("[{APP}] pasted transcript chars={}", text.chars().count());
    } else {
        println!("[{APP}] copied transcript chars={}", text.chars().count());
    }
    Ok(())
}

fn cancel() -> Result<()> {
    ensure_service()?;
    let _ = run_ctl(["cancel"], Duration::from_secs(5), false)?;
    let state = read_session()?;
    write_session(&SessionState {
        recording: false,
        session_id: state.session_id,
        updated_at: now_secs(),
    })?;
    println!("[{APP}] cancelled");
    Ok(())
}

fn unload_model() -> Result<()> {
    ensure_service()?;
    let _ = run_ctl(["unload-model"], Duration::from_secs(5), false)?;
    println!("[{APP}] unload requested");
    Ok(())
}

fn shutdown() -> Result<()> {
    let result = run_ctl(["shutdown"], Duration::from_secs(5), false)?;
    let state = read_session()?;
    write_session(&SessionState {
        recording: false,
        session_id: state.session_id,
        updated_at: now_secs(),
    })?;
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
    println!("Unload model:     {} unload-model", exe.display());
    println!("Stop daemon:      {} shutdown", exe.display());
    Ok(())
}

fn install_user_service() -> Result<()> {
    let unit_dir = config_home().join("systemd").join("user");
    fs::create_dir_all(&unit_dir).context("creating systemd user dir")?;
    let exe = std::env::current_exe().context("resolving current executable")?;
    let unit = unit_dir.join("simple-stt-linux.service");
    let body = format!(
        "[Unit]\nDescription=Simple STT Linux capture daemon\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} daemon\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    );
    fs::write(&unit, body).with_context(|| format!("writing {}", unit.display()))?;
    println!("Wrote {}", unit.display());
    println!("Run:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now simple-stt-linux.service");
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

fn wait_for_transcript(session_id: u64, after: u64, timeout_s: f64) -> Result<String> {
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
        for event in result.events {
            if event.kind == "transcript" && event.session_id == session_id.to_string() {
                return Ok(event.text);
            }
            if event.kind == "notice" && event.session_id == session_id.to_string() {
                eprintln!("[{APP}] {}: {}", event.level, event.text);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(String::new())
}

fn transform_text(text: &str) -> Result<String> {
    let cfg = AppConfig::load()?;
    let mut text = text.trim().to_owned();
    if cfg.remove_punctuation {
        text.retain(|ch| !".,!?;:".contains(ch));
    }
    if cfg.lowercase_output {
        text = text.to_lowercase();
    }
    if cfg.trailing_space && !text.is_empty() {
        text.push(' ');
    }
    Ok(text)
}

fn paste_text(text: &str, force_shift_insert: bool) -> Result<bool> {
    let old_clip = read_clipboard(false);
    let old_primary = read_clipboard(true);
    if !write_clipboard(text, false)? {
        bail!("No clipboard tool found. Install wl-clipboard on Wayland or xclip/xsel on X11.");
    }
    let _ = write_clipboard(text, true)?;
    std::thread::sleep(Duration::from_millis(80));
    let sent = send_paste_key(force_shift_insert)?;
    if !sent {
        eprintln!(
            "[{APP}] automatic paste failed; transcript is left in clipboard for manual paste"
        );
        return Ok(false);
    }
    let delay_ms = 250;
    std::thread::sleep(Duration::from_millis(delay_ms));
    if let Some(old_clip) = old_clip.as_deref() {
        let _ = write_clipboard(old_clip, false)?;
    }
    if let Some(old_primary) = old_primary.as_deref() {
        let _ = write_clipboard(old_primary, true)?;
    }
    Ok(true)
}

fn read_clipboard(primary: bool) -> Option<String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && command_exists("wl-paste") {
        let mut cmd = ProcessCommand::new("wl-paste");
        cmd.arg("--no-newline");
        if primary {
            cmd.arg("--primary");
        }
        return command_output(&mut cmd);
    }
    if command_exists("xclip") {
        let sel = if primary { "primary" } else { "clipboard" };
        return command_output(
            ProcessCommand::new("xclip")
                .arg("-selection")
                .arg(sel)
                .arg("-o"),
        );
    }
    if command_exists("xsel") {
        let flag = if primary { "--primary" } else { "--clipboard" };
        return command_output(ProcessCommand::new("xsel").arg(flag).arg("--output"));
    }
    None
}

fn write_clipboard(text: &str, primary: bool) -> Result<bool> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && command_exists("wl-copy") {
        let mut cmd = ProcessCommand::new("wl-copy");
        if primary {
            cmd.arg("--primary");
        }
        cmd.arg("--").arg(text);
        if cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return Ok(true);
        }
    }
    if command_exists("xclip") {
        let sel = if primary { "primary" } else { "clipboard" };
        return start_clipboard_owner(
            ProcessCommand::new("xclip").arg("-selection").arg(sel),
            text,
        );
    }
    if command_exists("xsel") {
        let flag = if primary { "--primary" } else { "--clipboard" };
        return start_clipboard_owner(ProcessCommand::new("xsel").arg(flag).arg("--input"), text);
    }
    Ok(false)
}

fn start_clipboard_owner(cmd: &mut ProcessCommand, text: &str) -> Result<bool> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting clipboard owner command")?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .context("writing clipboard content")?;
    }
    Ok(true)
}

fn send_paste_key(force_shift_insert: bool) -> Result<bool> {
    let use_shift_insert =
        force_shift_insert || std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland");
    if command_exists("ydotool") && command_exists("pidof") {
        let daemon_running = ProcessCommand::new("pidof")
            .arg("ydotoold")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if daemon_running {
            let args = if use_shift_insert {
                vec!["key", "42:1", "110:1", "110:0", "42:0"]
            } else {
                vec!["key", "29:1", "47:1", "47:0", "29:0"]
            };
            if run_quiet(ProcessCommand::new("ydotool").args(args)) {
                return Ok(true);
            }
        }
    }
    if command_exists("xdotool") {
        let key = if use_shift_insert {
            "shift+Insert"
        } else {
            "ctrl+v"
        };
        if run_quiet(ProcessCommand::new("xdotool").args(["key", "--clearmodifiers", key])) {
            return Ok(true);
        }
    }
    if command_exists("wtype") {
        let args = if use_shift_insert {
            vec!["-M", "shift", "-k", "Insert", "-m", "shift"]
        } else {
            vec!["-M", "ctrl", "-k", "v", "-m", "ctrl"]
        };
        if run_quiet(ProcessCommand::new("wtype").args(args)) {
            return Ok(true);
        }
    }
    Ok(false)
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
                    seq: parts[1].parse().unwrap_or(0),
                    kind: unescape(parts[2]),
                    session_id: parts[3].to_owned(),
                    level: parts[4].to_owned(),
                    text: unescape(parts[5]),
                });
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
    Ok(serde_json::from_str(&fs::read_to_string(session_file())?)?)
}

fn write_session(state: &SessionState) -> Result<()> {
    ensure_dirs()?;
    fs::write(session_file(), serde_json::to_string_pretty(state)? + "\n")?;
    Ok(())
}

fn read_seq() -> Result<u64> {
    if !seq_file().exists() {
        return Ok(0);
    }
    Ok(fs::read_to_string(seq_file())?.trim().parse().unwrap_or(0))
}

fn write_seq(seq: u64) -> Result<()> {
    ensure_dirs()?;
    fs::write(seq_file(), format!("{seq}\n"))?;
    Ok(())
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
    AppConfig::config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn next_session_id_is_monotonic() {
        assert!(next_session_id(42) >= 43);
    }
}
