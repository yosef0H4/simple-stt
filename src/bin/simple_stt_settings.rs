use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use simple_stt::capture::state::ServiceState;
use simple_stt::common::shell_protocol::{
    ClientMessage, ServerMessage, ShellCommand, ShellResponse, SHELL_PROTOCOL_VERSION,
};
use simple_stt::config::AppConfig;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX: &str = include_str!("../../web/settings/index.html");
const TOKENS: &str = include_str!("../../web/settings/tokens.css");
const CSS: &str = include_str!("../../web/settings/styles.css");
const JS: &str = include_str!("../../web/settings/app.js");
const MAX_BODY: usize = 2 * 1024 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Parser)]
#[command(name = "simple-stt-settings")]
struct Args {
    #[arg(long)]
    state_file: Option<PathBuf>,
    #[arg(long)]
    service_token: Option<String>,
    #[arg(long, hide = true)]
    no_browser: bool,
}

#[derive(Clone)]
struct CaptureConnection {
    state_file: PathBuf,
    token: String,
}

struct AppState {
    web_token: String,
    origin: String,
    capture: Option<CaptureConnection>,
    last_activity: Mutex<Instant>,
    closing: AtomicBool,
}

#[derive(Deserialize)]
struct SaveRequest {
    config: Value,
    expected_hash: String,
}

#[derive(Deserialize)]
struct ActionRequest {
    action: String,
    #[serde(default)]
    filename: String,
}

#[derive(Serialize, Deserialize)]
struct SettingsSession {
    pid: u32,
    origin: String,
    token: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("simple-stt-settings: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    if let Some(url) = reusable_session_url() {
        if !args.no_browser {
            open_url(&url)?;
        }
        println!("{url}");
        return Ok(());
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let address = listener.local_addr()?;
    let server = Server::from_listener(listener, None)
        .map_err(|error| anyhow::anyhow!("starting settings server: {error}"))?;
    let web_token = random_token();
    let origin = format!("http://127.0.0.1:{}", address.port());
    let capture = match (args.state_file, args.service_token) {
        (Some(state_file), Some(token)) => Some(CaptureConnection { state_file, token }),
        _ => None,
    };
    let state = Arc::new(AppState {
        web_token: web_token.clone(),
        origin: origin.clone(),
        capture,
        last_activity: Mutex::new(Instant::now()),
        closing: AtomicBool::new(false),
    });
    let url = format!("{origin}/#token={web_token}");
    write_session(&SettingsSession {
        pid: std::process::id(),
        origin: origin.clone(),
        token: web_token.clone(),
    })?;
    println!("{url}");
    if !args.no_browser {
        open_url(&url)?;
    }
    while !state.closing.load(Ordering::Relaxed) {
        if state.last_activity.lock().expect("activity lock").elapsed() > IDLE_TIMEOUT {
            break;
        }
        if let Some(request) = server.recv_timeout(Duration::from_millis(500))? {
            *state.last_activity.lock().expect("activity lock") = Instant::now();
            let state = Arc::clone(&state);
            std::thread::spawn(move || respond(request, &state));
        }
    }
    let _ = fs::remove_file(settings_session_path());
    Ok(())
}

fn respond(mut request: Request, state: &AppState) {
    let result = handle(&mut request, state);
    let response = match result {
        Ok(response) => response,
        Err(error) => json_response(
            StatusCode(400),
            &json!({"error": format!("{error:#}")}),
            state,
        ),
    };
    let _ = request.respond(response);
}

fn handle(request: &mut Request, state: &AppState) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    validate_host(request, state)?;
    let path = request.url().split('?').next().unwrap_or(request.url());
    if request.method() == &Method::Get {
        match path {
            "/" | "/index.html" => return Ok(asset(INDEX, "text/html; charset=utf-8", state)),
            "/tokens.css" => return Ok(asset(TOKENS, "text/css; charset=utf-8", state)),
            "/styles.css" => return Ok(asset(CSS, "text/css; charset=utf-8", state)),
            "/app.js" => return Ok(asset(JS, "text/javascript; charset=utf-8", state)),
            "/favicon.ico" => {
                return Ok(response_bytes(
                    StatusCode(204),
                    Vec::new(),
                    "image/x-icon",
                    state,
                ))
            }
            _ => {}
        }
    }
    authenticate(request, state)?;
    validate_origin(request, state)?;
    match (request.method(), path) {
        (&Method::Get, "/api/state") => state_response(state),
        (&Method::Get, "/api/health") => Ok(json_response(
            StatusCode(200),
            &json!({"ok":true,"pid":std::process::id()}),
            state,
        )),
        (&Method::Get, "/api/defaults") => Ok(json_response(
            StatusCode(200),
            &json!({"config": AppConfig::default()}),
            state,
        )),
        (&Method::Get, "/api/events") => events_response(request, state),
        (&Method::Post, "/api/normalize") => {
            let input: Value = serde_json::from_slice(&read_body(request)?)?;
            let config = AppConfig::normalize_json(&input);
            config.validate()?;
            Ok(json_response(
                StatusCode(200),
                &json!({"config": config}),
                state,
            ))
        }
        (&Method::Post, "/api/save") => save_response(request, state),
        (&Method::Post, "/api/action") => action_response(request, state),
        (&Method::Post, "/api/platform-action") => platform_action(request, state),
        (&Method::Post, "/api/hotkey-capture") => hotkey_capture_response(state),
        (&Method::Post, "/api/close") => {
            state.closing.store(true, Ordering::Relaxed);
            Ok(json_response(StatusCode(200), &json!({"ok": true}), state))
        }
        _ => Ok(json_response(
            StatusCode(405),
            &json!({"error":"method or route not allowed"}),
            state,
        )),
    }
}

fn hotkey_capture_response(state: &AppState) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    #[cfg(windows)]
    {
        let hotkey = capture_hotkey_with_ahk()?;
        Ok(json_response(
            StatusCode(200),
            &json!({"hotkey": hotkey}),
            state,
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(json_response(
            StatusCode(400),
            &json!({"error":"Shortcuts are assigned by your desktop environment"}),
            state,
        ))
    }
}

#[cfg(windows)]
fn capture_hotkey_with_ahk() -> Result<String> {
    let (runtime, script) = locate_hotkey_recorder()?;
    let output = std::env::temp_dir().join(format!(
        "simple-stt-hotkey-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let status = std::process::Command::new(runtime)
        .arg(script)
        .arg(&output)
        .status()
        .context("starting the AutoHotkey shortcut recorder")?;
    anyhow::ensure!(status.success(), "shortcut recording was cancelled");
    let hotkey = fs::read_to_string(&output)
        .context("reading the recorded shortcut")?
        .trim_start_matches('\u{feff}')
        .trim()
        .to_owned();
    let _ = fs::remove_file(output);
    anyhow::ensure!(!hotkey.is_empty(), "shortcut recording was cancelled");
    Ok(hotkey)
}

#[cfg(windows)]
fn locate_hotkey_recorder() -> Result<(PathBuf, PathBuf)> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .context("settings executable has no parent directory")?
        .to_path_buf();
    let cwd = std::env::current_dir()?;
    let scripts = [
        exe_dir.join("hotkey-recorder.ahk"),
        cwd.join("ahk").join("hotkey-recorder.ahk"),
    ];
    let script = scripts
        .into_iter()
        .find(|path| path.is_file())
        .context("hotkey-recorder.ahk was not found")?;
    let mut runtimes = vec![exe_dir.join("AutoHotkey64.exe")];
    for variable in ["ProgramFiles", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(variable) {
            let base = PathBuf::from(base);
            runtimes.push(base.join("AutoHotkey").join("v2").join("AutoHotkey64.exe"));
            runtimes.push(
                base.join("Programs")
                    .join("AutoHotkey")
                    .join("v2")
                    .join("AutoHotkey64.exe"),
            );
        }
    }
    let runtime = runtimes
        .into_iter()
        .find(|path| path.is_file())
        .context("AutoHotkey v2 was not found")?;
    Ok((runtime, script))
}

fn state_response(state: &AppState) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let path = AppConfig::config_path();
    let mut raw = fs::read(&path).unwrap_or_default();
    let config = match AppConfig::load() {
        Ok(config) => config,
        Err(error) => {
            return Ok(json_response(
                StatusCode(200),
                &json!({
                    "config":Value::Null,
                    "config_hash":hash_bytes(&raw),
                    "config_path":path,
                    "config_error":format!("{error:#}"),
                    "platform":if cfg!(windows){"windows"}else if cfg!(target_os="linux"){"linux"}else{"other"},
                    "service_online":false,
                    "microphones":[],
                    "models":[]
                }),
                state,
            ))
        }
    };
    raw = fs::read(&path)?;
    let models = simple_stt::models::catalog_for_config(&config)
        .into_iter()
        .map(|model| json!({"family":model.family,"quant":model.quant,"file":model.file,"size_mb":model.size_mb,"recommended":model.recommended,"installed":model.installed,"languages":model.languages}))
        .collect::<Vec<_>>();
    let service_online = capture_request(state, ShellCommand::Ping).is_ok();
    let microphones = if service_online {
        capture_request(state, ShellCommand::ListInputs)
            .ok()
            .map(|response| parse_indexed_values(&response, "input"))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let shortcut_state = linux_shortcut_state();
    Ok(json_response(
        StatusCode(200),
        &json!({
            "config":config,
            "config_hash":hash_bytes(&raw),
            "config_path":AppConfig::config_path(),
            "resolved_runtime_dir":config.parakeet_runtime_dir_path(),
            "resolved_model_dir":config.model_dir_path(),
            "platform":if cfg!(windows){"windows"}else if cfg!(target_os="linux"){"linux"}else{"other"},
            "service_online":service_online,
            "shortcut_state":shortcut_state,
            "linux_automation":linux_automation_state(),
            "microphones":microphones,
            "models":models
        }),
        state,
    ))
}

#[cfg(target_os = "linux")]
fn linux_shortcut_state() -> Value {
    fs::read(AppConfig::local_data_dir().join("linux-shortcuts.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_else(|| json!({}))
}

#[cfg(target_os = "linux")]
fn linux_automation_state() -> Value {
    let exists = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
    };
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let ydotool_daemon = Command::new("pidof")
        .arg("ydotoold")
        .output()
        .is_ok_and(|output| output.status.success());
    let native = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources/bin/linux-fast-paste")
        .is_file();
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let release_value = |key: &str| {
        os_release
            .lines()
            .find_map(|line| {
                let (candidate, value) = line.split_once('=')?;
                (candidate == key).then(|| value.trim_matches('"').to_owned())
            })
            .unwrap_or_default()
    };
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Unknown desktop".into());
    let desktop_lower = desktop.to_ascii_lowercase();
    let recommended = if !wayland && exists("xdotool") {
        "xdotool"
    } else if wayland
        && (desktop_lower.contains("kde") || desktop_lower.contains("plasma"))
        && native
    {
        "native"
    } else if wayland && desktop_lower.contains("gnome") && ydotool_daemon {
        "ydotool"
    } else if wayland && exists("wtype") {
        "wtype"
    } else if ydotool_daemon {
        "ydotool"
    } else if native {
        "native"
    } else {
        "clipboard_only"
    };
    json!({
        "session": if wayland { "Wayland" } else { "X11" },
        "desktop": desktop,
        "distro": release_value("PRETTY_NAME"),
        "distro_id": release_value("ID"),
        "wl_clipboard": exists("wl-copy") && exists("wl-paste"),
        "wtype": exists("wtype"),
        "ydotool": exists("ydotool"),
        "ydotool_daemon": ydotool_daemon,
        "xdotool": exists("xdotool"),
        "native": native,
        "recommended": recommended,
        "start_command": "systemctl --user start simple-stt-linux.service",
        "stop_command": "simple-stt-linux shutdown"
    })
}

#[cfg(not(target_os = "linux"))]
fn linux_automation_state() -> Value {
    json!({})
}

#[cfg(not(target_os = "linux"))]
fn linux_shortcut_state() -> Value {
    json!({})
}

fn save_response(
    request: &mut Request,
    state: &AppState,
) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let body: SaveRequest = serde_json::from_slice(&read_body(request)?)?;
    let path = AppConfig::config_path();
    let current = fs::read(&path).unwrap_or_default();
    anyhow::ensure!(
        hash_bytes(&current) == body.expected_hash,
        "config changed outside Settings; reload before saving"
    );
    let config = AppConfig::normalize_json(&body.config);
    config.validate()?;
    config.save()?;
    let raw = fs::read(path)?;
    let reloaded = capture_request(state, ShellCommand::ReloadConfig).is_ok();
    Ok(json_response(
        StatusCode(200),
        &json!({"config":config,"config_hash":hash_bytes(&raw),"reloaded":reloaded}),
        state,
    ))
}

fn action_response(
    request: &mut Request,
    state: &AppState,
) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let body: ActionRequest = serde_json::from_slice(&read_body(request)?)?;
    let command = match body.action.as_str() {
        "refresh_models" => ShellCommand::RefreshModels,
        "download_model" => ShellCommand::DownloadModel {
            filename: body.filename,
        },
        "remove_model" => ShellCommand::RemoveModel {
            filename: body.filename,
        },
        "test_model" => ShellCommand::TestModel,
        _ => anyhow::bail!("unsupported service action"),
    };
    let response = capture_request(state, command)?;
    anyhow::ensure!(response.ok, "{}", response.message);
    Ok(json_response(
        StatusCode(200),
        &json!({"message":response.message,"values":response.values}),
        state,
    ))
}

fn events_response(
    request: &Request,
    state: &AppState,
) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let after = request
        .url()
        .split_once("after=")
        .and_then(|(_, value)| value.split('&').next())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let response = capture_request(state, ShellCommand::PollEvents { after_seq: after })?;
        if !response.events.is_empty() || Instant::now() >= deadline {
            let payload = serde_json::to_string(&json!({"events":response.events}))?;
            return Ok(response_bytes(
                StatusCode(200),
                format!("event: service-events\ndata: {payload}\n\n").into_bytes(),
                "text/event-stream; charset=utf-8",
                state,
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn platform_action(
    request: &mut Request,
    state: &AppState,
) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let body: ActionRequest = serde_json::from_slice(&read_body(request)?)?;
    if body.action == "focused_app" {
        std::thread::sleep(Duration::from_secs(3));
        let identity = focused_app_identity()?;
        return Ok(json_response(
            StatusCode(200),
            &json!({"message":"Focused app detected","app_id":identity}),
            state,
        ));
    }
    match body.action.as_str() {
        "open_config" => open_path(&AppConfig::config_path())?,
        "open_config_folder" => open_path(
            AppConfig::config_path()
                .parent()
                .context("config has no parent")?,
        )?,
        "configure_shortcuts" => configure_shortcuts()?,
        "sync_shortcuts" => configure_shortcuts()?,
        _ => anyhow::bail!("unsupported platform action"),
    }
    Ok(json_response(
        StatusCode(200),
        &json!({"message":"Request opened"}),
        state,
    ))
}

#[cfg(target_os = "linux")]
fn focused_app_identity() -> Result<String> {
    let exe = std::env::current_exe()?;
    let helper = [
        exe.parent()
            .unwrap_or(Path::new("."))
            .join("linux-fast-paste"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/bin/linux-fast-paste"),
        PathBuf::from("resources/bin/linux-fast-paste"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .context("focused-app detector is not installed")?;
    let output = Command::new(helper).arg("--active-app").output()?;
    anyhow::ensure!(
        output.status.success(),
        "The focused app does not expose an identity; enter it manually"
    );
    let identity = String::from_utf8(output.stdout)?.trim().to_owned();
    anyhow::ensure!(!identity.is_empty(), "The focused app identity is empty");
    Ok(identity)
}

#[cfg(windows)]
fn focused_app_identity() -> Result<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let window = GetForegroundWindow();
        anyhow::ensure!(!window.is_null(), "No foreground application was found");
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        anyhow::ensure!(pid != 0, "The foreground application has no process ID");
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        anyhow::ensure!(
            !process.is_null(),
            "The foreground application cannot be inspected"
        );
        let mut path = vec![0_u16; 32_768];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
        CloseHandle(process);
        anyhow::ensure!(ok != 0, "The foreground executable name could not be read");
        let path = String::from_utf16(&path[..len as usize])?;
        let identity = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .context("The foreground executable has no file name")?
            .to_owned();
        Ok(identity)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn focused_app_identity() -> Result<String> {
    anyhow::bail!("Focused-app detection is unavailable on this platform")
}

fn capture_request(state: &AppState, command: ShellCommand) -> Result<ShellResponse> {
    let capture = state
        .capture
        .as_ref()
        .context("capture service is offline")?;
    let service = ServiceState::load(&capture.state_file)?;
    anyhow::ensure!(
        service.protocol == SHELL_PROTOCOL_VERSION,
        "capture protocol mismatch"
    );
    let mut stream = TcpStream::connect_timeout(&service.address.parse()?, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(6)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    write_json(
        &mut stream,
        &ClientMessage::Hello {
            protocol: SHELL_PROTOCOL_VERSION,
            token: capture.token.clone(),
        },
    )?;
    match read_json::<ServerMessage>(&mut reader)? {
        ServerMessage::HelloAck { .. } => {}
        other => anyhow::bail!("capture handshake failed: {other:?}"),
    }
    let request_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    write_json(
        &mut stream,
        &ClientMessage::Command {
            request_id,
            command,
        },
    )?;
    match read_json::<ServerMessage>(&mut reader)? {
        ServerMessage::Response {
            request_id: actual,
            response,
        } if actual == request_id => Ok(response),
        ServerMessage::Error { code, message } => anyhow::bail!("{code}: {message}"),
        other => anyhow::bail!("unexpected capture response: {other:?}"),
    }
}

fn parse_indexed_values(response: &ShellResponse, prefix: &str) -> Vec<Value> {
    response
        .values
        .iter()
        .filter_map(|(key, name)| {
            let index = key
                .strip_prefix(&format!("{prefix}."))?
                .strip_suffix(".label")?;
            let id = response
                .values
                .get(&format!("{prefix}.{index}.id"))
                .cloned()
                .unwrap_or_else(|| name.clone());
            Some(json!({"id":id,"name":name.clone()}))
        })
        .collect()
}

fn read_body(request: &mut Request) -> Result<Vec<u8>> {
    let length = request.body_length().unwrap_or(0);
    anyhow::ensure!(length <= MAX_BODY, "request body is too large");
    let mut body = Vec::with_capacity(length);
    request
        .as_reader()
        .take((MAX_BODY + 1) as u64)
        .read_to_end(&mut body)?;
    anyhow::ensure!(body.len() <= MAX_BODY, "request body is too large");
    Ok(body)
}

fn authenticate(request: &Request, state: &AppState) -> Result<()> {
    let token = header(request, "X-Simple-STT-Token").unwrap_or_default();
    anyhow::ensure!(
        constant_time_eq(token.as_bytes(), state.web_token.as_bytes()),
        "unauthorized"
    );
    Ok(())
}

fn validate_host(request: &Request, state: &AppState) -> Result<()> {
    let expected = state.origin.trim_start_matches("http://");
    anyhow::ensure!(
        header(request, "Host") == Some(expected),
        "invalid Host header"
    );
    Ok(())
}

fn validate_origin(request: &Request, state: &AppState) -> Result<()> {
    if let Some(origin) = header(request, "Origin") {
        anyhow::ensure!(origin == state.origin, "invalid Origin header");
    }
    Ok(())
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}

fn asset(body: &str, content_type: &str, state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    response_bytes(
        StatusCode(200),
        body.as_bytes().to_vec(),
        content_type,
        state,
    )
}

fn json_response(
    status: StatusCode,
    value: &Value,
    state: &AppState,
) -> Response<std::io::Cursor<Vec<u8>>> {
    response_bytes(
        status,
        serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec()),
        "application/json; charset=utf-8",
        state,
    )
}

fn response_bytes(
    status: StatusCode,
    body: Vec<u8>,
    content_type: &str,
    state: &AppState,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(body).with_status_code(status);
    for (name,value) in [
        ("Content-Type",content_type),
        ("Cache-Control","no-store"),
        ("X-Content-Type-Options","nosniff"),
        ("Referrer-Policy","no-referrer"),
        ("Content-Security-Policy","default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"),
        ("Cross-Origin-Resource-Policy","same-origin"),
    ] { response.add_header(Header::from_bytes(name,value).expect("valid header")); }
    response.add_header(
        Header::from_bytes("Access-Control-Allow-Origin", state.origin.as_str())
            .expect("valid origin"),
    );
    response
}

fn random_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn settings_session_path() -> PathBuf {
    AppConfig::state_dir().join("settings-session.json")
}

fn write_session(session: &SettingsSession) -> Result<()> {
    let path = settings_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec(session)?)?;
    Ok(())
}

fn reusable_session_url() -> Option<String> {
    let session: SettingsSession =
        serde_json::from_slice(&fs::read(settings_session_path()).ok()?).ok()?;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .ok()?
        .get(format!("{}/api/health", session.origin))
        .header("X-Simple-STT-Token", &session.token)
        .send()
        .ok()?;
    if response.status().is_success() {
        Some(format!("{}/#token={}", session.origin, session.token))
    } else {
        None
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b
    }
    diff == 0
}

fn write_json<T: serde::Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Result<T> {
    let mut line = String::new();
    anyhow::ensure!(
        reader.read_line(&mut line)? > 0,
        "capture closed connection"
    );
    Ok(serde_json::from_str(line.trim_end())?)
}

#[cfg(windows)]
fn open_url(url: &str) -> Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = url
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(
        result as isize > 32,
        "Windows failed to open the default browser (ShellExecuteW code {})",
        result as isize
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn open_url(url: &str) -> Result<()> {
    Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn open_url(_url: &str) -> Result<()> {
    anyhow::bail!("opening a browser is unsupported")
}

fn open_path(path: &Path) -> Result<()> {
    open_url(path.to_str().context("path is not Unicode")?)
}

#[cfg(target_os = "linux")]
fn configure_shortcuts() -> Result<()> {
    let executable = std::env::current_exe()?
        .parent()
        .context("settings executable has no directory")?
        .join("simple-stt-linux");
    Command::new(executable)
        .arg("configure-shortcuts")
        .spawn()
        .context("requesting Linux shortcut configuration")?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn configure_shortcuts() -> Result<()> {
    anyhow::bail!("system shortcut configuration is only available on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_is_256_bits() {
        assert_eq!(random_token().len(), 64)
    }
    #[test]
    fn constant_time_comparison_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }
    #[test]
    fn assets_are_bundled() {
        assert!(INDEX.contains("Audio &amp; models"));
        assert!(TOKENS.contains("--color-accent"));
        assert!(JS.contains("model_download_progress"));
        assert!(JS.contains("Search language or model"));
    }
}
