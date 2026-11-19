mod activity;
mod chunk_stream;
mod lease_timer;
mod media_routing;
mod model_events;
#[cfg(target_os = "linux")]
mod portal_identity;
#[cfg(target_os = "linux")]
mod portal_input;
mod privacy;
mod read_along;
mod read_along_capture;
mod reading_hint;
#[cfg(target_os = "linux")]
mod region_capture;
mod repeat;
mod request_lifetime;
#[cfg(target_os = "linux")]
mod selection_watch;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(any(unix, windows))]
use reader_core::config::data_dir;
#[cfg(target_os = "linux")]
use reader_core::config::SelectionCopyShortcut;
use reader_core::config::{
    config_path, AppConfig, ConfigStore, DevicePreference, PlaybackSettings, SpeechProvider,
};
use reader_core::generation::{GenerationGate, WorkerIncarnation};
use reader_core::text::{chunk_progress, plan_units, SpeechUnit, MAX_TEXT_BYTES};
use reader_core::wire::{
    read_json_frame, read_worker_frame, write_json_frame, WorkerEvent, WorkerFrame, WorkerRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "readerd", about = "Simple TTS desktop controller")]
struct Args {
    #[command(subcommand)]
    command: Mode,
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}
#[derive(Subcommand)]
enum Mode {
    Daemon {
        #[arg(long)]
        foreground: bool,
        #[arg(long)]
        no_desktop: bool,
    },
    Server,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum LocalRequest {
    ReadStart {
        id: String,
        source: String,
    },
    ReadChunk {
        id: String,
        text: String,
    },
    ReadFinish {
        id: String,
    },
    ReadImage {
        path: String,
    },
    RenderFile {
        input: String,
        output: String,
    },
    OcrFile {
        path: String,
    },
    Action {
        action: String,
    },
    #[cfg(target_os = "linux")]
    Input {
        id: String,
        action: portal_input::Action,
    },
    #[cfg(target_os = "linux")]
    CancelInput {
        id: String,
    },
    Status,
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
struct LocalReply {
    ok: bool,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<serde_json::Value>,
}
impl LocalReply {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            state: None,
        }
    }
    fn state(value: serde_json::Value) -> Self {
        Self {
            ok: true,
            message: "controller ready".into(),
            state: Some(value),
        }
    }
    fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            state: None,
        }
    }
}
#[derive(Debug)]
struct Draft {
    source: String,
    text: String,
    created: Instant,
    generation: u64,
    repeat_revision: u64,
}
#[derive(Debug, Clone)]
struct Session {
    job_id: String,
    generation: u64,
    source: String,
    units: Vec<SpeechUnit>,
    preparation: Arc<chunk_stream::Progress>,
}

#[derive(Clone)]
struct App {
    inner: Arc<AppInner>,
}
struct IdleLeaseOnDrop<'a>(&'a App);
impl Drop for IdleLeaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.schedule_idle_lease_if_current();
    }
}
struct ClipboardSuppressionGuard<'a>(&'a App);
impl Drop for ClipboardSuppressionGuard<'_> {
    fn drop(&mut self) {
        self.0.rebaseline_clipboard();
        self.0
            .inner
            .clipboard_suppression
            .fetch_sub(1, Ordering::AcqRel);
    }
}
struct AppInner {
    store: ConfigStore,
    model_events: model_events::ModelEvents,
    route_language: Mutex<Option<String>>,
    activity: Mutex<activity::Activity>,
    ocr_activity: Mutex<activity::Activity>,
    speculative_epoch: AtomicU64,
    copy_epoch: AtomicU64,
    region_gate: Mutex<()>,
    generation: GenerationGate,
    drafts: Mutex<HashMap<String, Draft>>,
    draft_timer: lease_timer::LeaseTimer,
    session: Mutex<Option<Session>>,
    audio: Arc<AudioOutput>,
    tts: Arc<WorkerSupervisor>,
    media_bridge: Mutex<Option<MediaBridge>>,
    warmups: Mutex<HashMap<String, (String, Instant)>>,
    server_jobs: Mutex<HashMap<String, Arc<ServerJob>>>,
    server_job_gate: Mutex<()>,
    active_server_job: Mutex<Option<String>>,
    server_job_generation: AtomicU64,
    server_unload_epoch: AtomicU64,
    server_idle_epoch: AtomicU64,
    server_idle_timer: lease_timer::LeaseTimer,
    capture_kill: Mutex<Option<ChildKill>>,
    intent_gate: Mutex<()>,
    idle_deadline: Mutex<Option<Instant>>,
    idle_lease_epoch: AtomicU64,
    idle_timer: lease_timer::LeaseTimer,
    pause_timer: lease_timer::LeaseTimer,
    ocr_timer: lease_timer::LeaseTimer,
    capture_epoch: AtomicU64,
    ocr_prewarm_epoch: AtomicU64,
    #[cfg(target_os = "linux")]
    clipboard_owner: Mutex<Option<(ChildKill, Child)>>,
    clipboard_listening: AtomicBool,
    clipboard_listener_epoch: AtomicU64,
    clipboard_baseline: Mutex<Option<u64>>,
    clipboard_change_gate: Mutex<()>,
    clipboard_change_running: AtomicBool,
    clipboard_change_pending: AtomicBool,
    clipboard_suppression: AtomicUsize,
    clipboard_read_generation: AtomicU64,
    #[cfg(target_os = "linux")]
    clipboard_watch_kill: Mutex<Option<ChildKill>>,
    #[cfg(target_os = "linux")]
    tray_handle: Mutex<Option<ksni::blocking::Handle<Tray>>>,
    pause_lifecycle_gate: Mutex<()>,
    pause_lease_epoch: AtomicU64,
    paused_restart_generation: AtomicU64,
    ocr_kill: Mutex<Option<(ChildKill, bool)>>,
    ocr_worker: Mutex<Option<OcrWorker>>,
    ocr_residency: RwLock<Option<reader_core::residency::Estimate>>,
    ocr_epoch: AtomicU64,
    ocr_gate: Mutex<()>,
    shutdown: AtomicBool,
    server: Mutex<Option<Arc<AtomicBool>>>,
    hud: Mutex<Option<Arc<AtomicBool>>>,
    desktop_enabled: AtomicBool,
    notice: Mutex<ControllerNotice>,
    read_pending: AtomicBool,
    read_along_active: AtomicBool,
    read_along: Mutex<Option<ReadAlongRun>>,
    repeat_action: Mutex<repeat::RepeatActionState>,
}

struct MediaBridge {
    child: Child,
    requests: Sender<MediaBridgeRequest>,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

struct MediaBridgeRequest {
    value: serde_json::Value,
    reply: Sender<Result<serde_json::Value, String>>,
}

impl Drop for MediaBridge {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            #[cfg(target_os = "linux")]
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            #[cfg(windows)]
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

#[cfg(windows)]
fn create_media_bridge_job(child: &mut Child) -> Result<windows_sys::Win32::Foundation::HANDLE> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    anyhow::ensure!(!job.is_null(), "creating media bridge job object failed");
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    let assigned = configured != 0
        && unsafe {
            AssignProcessToJobObject(job, child.as_raw_handle().cast::<std::ffi::c_void>().cast())
        } != 0;
    if !assigned {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(job) };
        anyhow::bail!("assigning media bridge to its private job object failed");
    }
    Ok(job)
}
fn read_along_input(
    settings: &reader_core::config::ReadAlongSettings,
    target: &read_along_capture::Target,
) -> read_along_capture::AdvanceAction {
    match settings.action {
        reader_core::config::ReadAlongAction::Key => settings
            .sequence
            .clone()
            .map(read_along_capture::AdvanceAction::Sequence)
            .unwrap_or_else(|| {
                read_along_capture::AdvanceAction::Key(settings.advance_key.clone())
            }),
        reader_core::config::ReadAlongAction::Click => read_along_capture::AdvanceAction::Click {
            x: target.region.width / 2,
            y: target.region.height / 2,
        },
    }
}
fn receive_read_along<T>(
    receiver: std::sync::mpsc::Receiver<Result<T, String>>,
    current: &dyn Fn() -> bool,
) -> Result<T> {
    loop {
        anyhow::ensure!(current(), "read-along was canceled");
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => return result.map_err(anyhow::Error::msg),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => return Err(error.into()),
        }
    }
}
fn wait_read_along_settle(
    advanced: Instant,
    delay_ms: u16,
    current: &dyn Fn() -> bool,
) -> Result<()> {
    loop {
        anyhow::ensure!(current(), "read-along was canceled");
        let remaining = read_along::settle_remaining(delay_ms, advanced.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}
struct ReadAlongRun {
    controller: read_along::Controller,
    target: read_along_capture::Target,
    app_generation: u64,
    settings: reader_core::config::ReadAlongSettings,
    prepared: Option<PreparedReadAlongPage>,
    next_activity: Option<ReadAlongNextActivity>,
    next_progress: Option<Arc<chunk_stream::Progress>>,
    first_advance: Option<std::sync::mpsc::Receiver<Result<Instant, String>>>,
    preparation: read_along::Preparation,
    active_stream: Option<Arc<AtomicU64>>,
}
#[derive(Clone, Serialize)]
struct ReadAlongNextActivity {
    phase: &'static str,
    #[serde(skip)]
    waiting_until: Option<Instant>,
}
struct PreparedReadAlongPage {
    text: String,
    first: PreparedReadAlongChunk,
    progress: Arc<chunk_stream::Progress>,
    remaining: chunk_stream::Receiver<Result<Option<PreparedReadAlongChunk>, String>>,
    playing_generation: Arc<AtomicU64>,
}
struct PreparedReadAlongChunk {
    unit: SpeechUnit,
    samples: Vec<i16>,
    route: AppConfig,
}
struct ControllerNotice {
    text: String,
    revision: u64,
    expires: Option<Instant>,
}
enum ClipboardPayload {
    Text(String),
    Image {
        bytes: Vec<u8>,
        extension: &'static str,
    },
}
struct ClipboardSnapshot {
    fingerprint: u64,
    payload: Option<ClipboardPayload>,
}
const MAX_CLIPBOARD_IMAGE_BYTES: usize = 12 * 1024 * 1024;
#[cfg(target_os = "linux")]
const CLIPBOARD_READ_TIMEOUT: Duration = Duration::from_secs(2);

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn clipboard_fingerprint(kind: &[u8], bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn empty_clipboard_snapshot() -> ClipboardSnapshot {
    ClipboardSnapshot {
        fingerprint: clipboard_fingerprint(b"empty", b""),
        payload: None,
    }
}

fn clipboard_snapshot_from_bytes(
    kind: &'static str,
    bytes: Vec<u8>,
    extension: Option<&'static str>,
) -> ClipboardSnapshot {
    let fingerprint = clipboard_fingerprint(kind.as_bytes(), &bytes);
    let payload = if let Some(extension) = extension {
        (!bytes.is_empty() && bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES)
            .then_some(ClipboardPayload::Image { bytes, extension })
    } else {
        String::from_utf8(bytes).ok().and_then(|text| {
            (!text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES)
                .then_some(ClipboardPayload::Text(text))
        })
    };
    ClipboardSnapshot {
        fingerprint,
        payload,
    }
}

#[cfg(any(target_os = "linux", test))]
fn clipboard_has_format(types: &str, name: &str) -> bool {
    types
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(name))
}

#[cfg(any(target_os = "linux", test))]
fn clipboard_kde_hint_is_secret(value: &[u8]) -> bool {
    let value = value.trim_ascii();
    let value = value.strip_suffix(&[0]).unwrap_or(value).trim_ascii();
    value.eq_ignore_ascii_case(b"secret")
}

#[cfg(any(windows, test))]
fn clipboard_dword_disallows(value: &[u8]) -> bool {
    value.len() >= 4 && value[..4] == [0, 0, 0, 0]
}

#[cfg(target_os = "linux")]
fn clipboard_command_bytes(mut command: Command, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read as _;
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().context("starting clipboard reader")?;
    let stdout = child
        .stdout
        .take()
        .context("clipboard reader has no output")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout
            .take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + CLIPBOARD_READ_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().context("waiting for clipboard reader")? {
            let bytes = reader
                .join()
                .map_err(|_| anyhow::anyhow!("clipboard reader thread failed"))?;
            anyhow::ensure!(status.success(), "clipboard data is unavailable");
            anyhow::ensure!(
                bytes.len() <= limit,
                "clipboard data exceeds its size limit"
            );
            return Ok(bytes);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            bail!("clipboard read timed out");
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[cfg(target_os = "linux")]
fn read_clipboard_snapshot() -> Result<ClipboardSnapshot> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let types = if wayland {
        let mut command = Command::new("wl-paste");
        command.arg("--list-types");
        clipboard_command_bytes(command, 16 * 1024).unwrap_or_default()
    } else {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard", "-o", "-target", "TARGETS"]);
        clipboard_command_bytes(command, 16 * 1024).unwrap_or_default()
    };
    let listed_types = String::from_utf8_lossy(&types);
    if clipboard_has_format(&listed_types, "x-kde-passwordManagerHint") {
        let hint_type = listed_types
            .lines()
            .map(str::trim)
            .find(|name| name.eq_ignore_ascii_case("x-kde-passwordManagerHint"))
            .unwrap_or("x-kde-passwordManagerHint");
        let mut command = Command::new(if wayland { "wl-paste" } else { "xclip" });
        if wayland {
            command.args(["--no-newline", "--type", hint_type]);
        } else {
            command.args(["-selection", "clipboard", "-o", "-target", hint_type]);
        }
        // If the advertised hint cannot be read, avoid requesting content.
        let hint = clipboard_command_bytes(command, 128)?;
        if clipboard_kde_hint_is_secret(&hint) {
            return Ok(empty_clipboard_snapshot());
        }
    }
    let type_list = String::from_utf8_lossy(&types).to_ascii_lowercase();
    for (mime, extension) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
    ] {
        if type_list.contains(mime) {
            let command = if wayland {
                let mut command = Command::new("wl-paste");
                command.args(["--no-newline", "--type", mime]);
                command
            } else {
                let mut command = Command::new("xclip");
                command.args(["-selection", "clipboard", "-o", "-target", mime]);
                command
            };
            let bytes = clipboard_command_bytes(command, MAX_CLIPBOARD_IMAGE_BYTES)?;
            return Ok(clipboard_snapshot_from_bytes(mime, bytes, Some(extension)));
        }
    }
    let text_type = [
        ("text/plain;charset=utf-8", "text/plain;charset=utf-8"),
        ("text/plain", "text/plain"),
        ("UTF8_STRING", "utf8_string"),
        ("STRING", "string"),
    ]
    .into_iter()
    .find(|(_, candidate)| type_list.contains(candidate))
    .map(|(request_name, _)| request_name);
    if let Some(text_type) = text_type {
        let command = if wayland {
            let mut command = Command::new("wl-paste");
            command.args(["--no-newline", "--type", text_type]);
            command
        } else {
            let mut command = Command::new("xclip");
            command.args(["-selection", "clipboard", "-o", "-target", text_type]);
            command
        };
        let bytes = clipboard_command_bytes(command, MAX_TEXT_BYTES)?;
        return Ok(clipboard_snapshot_from_bytes(text_type, bytes, None));
    }
    Ok(empty_clipboard_snapshot())
}

#[cfg(windows)]
fn read_clipboard_snapshot() -> Result<ClipboardSnapshot> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const CF_UNICODETEXT: u32 = 13;
    anyhow::ensure!(
        unsafe { OpenClipboard(std::ptr::null_mut()) } != 0,
        "clipboard is busy"
    );
    let result = (|| {
        // Check explicit privacy formats before requesting the text or image.
        for (name, presence_only) in [
            ("Clipboard Viewer Ignore", true),
            ("ExcludeClipboardContentFromMonitorProcessing", true),
            ("CanIncludeInClipboardHistory", false),
            ("CanUploadToCloudClipboard", false),
        ] {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
            if format == 0 || unsafe { IsClipboardFormatAvailable(format) } == 0 {
                continue;
            }
            if presence_only {
                return Ok(empty_clipboard_snapshot());
            }
            let handle = unsafe { GetClipboardData(format) };
            // Fail closed on malformed or unavailable privacy hints.
            if handle.is_null() || unsafe { GlobalSize(handle) } < 4 {
                return Ok(empty_clipboard_snapshot());
            }
            let ptr = unsafe { GlobalLock(handle) }.cast::<u8>();
            if ptr.is_null() {
                return Ok(empty_clipboard_snapshot());
            }
            let disallows =
                clipboard_dword_disallows(unsafe { std::slice::from_raw_parts(ptr, 4) });
            unsafe { GlobalUnlock(handle) };
            if disallows {
                return Ok(empty_clipboard_snapshot());
            }
        }
        if unsafe { IsClipboardFormatAvailable(CF_DIBV5) } != 0
            || unsafe { IsClipboardFormatAvailable(CF_DIB) } != 0
        {
            let format = if unsafe { IsClipboardFormatAvailable(CF_DIBV5) } != 0 {
                CF_DIBV5
            } else {
                CF_DIB
            };
            let handle = unsafe { GetClipboardData(format) };
            if handle.is_null() {
                return Ok(empty_clipboard_snapshot());
            }
            let size = unsafe { GlobalSize(handle) };
            anyhow::ensure!(
                size > 0 && size <= MAX_CLIPBOARD_IMAGE_BYTES,
                "clipboard image exceeds its size limit"
            );
            let ptr = unsafe { GlobalLock(handle) }.cast::<u8>();
            anyhow::ensure!(!ptr.is_null(), "clipboard image could not be read");
            let dib = unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec();
            unsafe { GlobalUnlock(handle) };
            anyhow::ensure!(dib.len() >= 40, "clipboard bitmap header is incomplete");
            let header_size = u32::from_le_bytes(dib[0..4].try_into().unwrap()) as usize;
            anyhow::ensure!(
                header_size >= 40 && header_size <= dib.len(),
                "clipboard bitmap header is unsupported"
            );
            let width = i32::from_le_bytes(dib[4..8].try_into().unwrap()).unsigned_abs();
            let height = i32::from_le_bytes(dib[8..12].try_into().unwrap()).unsigned_abs();
            anyhow::ensure!(
                width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 20_000_000,
                "clipboard bitmap exceeds the 20 megapixel limit"
            );
            let mut bitmap = Vec::with_capacity(dib.len() + 14);
            bitmap.extend_from_slice(b"BM");
            bitmap.extend_from_slice(&((dib.len() + 14) as u32).to_le_bytes());
            bitmap.extend_from_slice(&[0; 4]);
            bitmap.extend_from_slice(&14u32.to_le_bytes());
            bitmap.extend_from_slice(&dib);
            let decoded = image::load_from_memory_with_format(&bitmap, image::ImageFormat::Bmp)
                .context("clipboard bitmap could not be decoded")?;
            let mut png = std::io::Cursor::new(Vec::new());
            decoded.write_to(&mut png, image::ImageFormat::Png)?;
            let bytes = png.into_inner();
            anyhow::ensure!(
                bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES,
                "clipboard image is too large"
            );
            return Ok(clipboard_snapshot_from_bytes(
                "image/png",
                bytes,
                Some("png"),
            ));
        }
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } != 0 {
            let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
            if handle.is_null() {
                return Ok(empty_clipboard_snapshot());
            }
            let size = unsafe { GlobalSize(handle) };
            anyhow::ensure!(
                size > 0 && size <= MAX_TEXT_BYTES * 2 + 2,
                "clipboard text is too large"
            );
            let ptr = unsafe { GlobalLock(handle) }.cast::<u16>();
            anyhow::ensure!(!ptr.is_null(), "clipboard text could not be read");
            let units = unsafe { std::slice::from_raw_parts(ptr, size / 2) };
            let end = units
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(units.len());
            let decoded = String::from_utf16(&units[..end]);
            unsafe { GlobalUnlock(handle) };
            let text = decoded.context("clipboard text is invalid UTF-16")?;
            return Ok(clipboard_snapshot_from_bytes(
                "text/plain",
                text.into_bytes(),
                None,
            ));
        }
        Ok(empty_clipboard_snapshot())
    })();
    unsafe { CloseClipboard() };
    result
}

#[cfg(not(any(target_os = "linux", windows)))]
fn read_clipboard_snapshot() -> Result<ClipboardSnapshot> {
    bail!("clipboard listening is not available on this platform")
}

#[cfg(target_os = "linux")]
fn start_clipboard_listener(app: App, epoch: u64) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let executable = sibling("readerctl")?;
        let mut child = Command::new("wl-paste")
            .arg("--watch")
            .arg(executable)
            .arg("clipboard-changed")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting Wayland clipboard watcher (install wl-clipboard)")?;
        let identity = match ChildKill::new(child.id(), Uuid::new_v4().to_string()) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let incarnation = identity.incarnation.clone();
        *app.inner
            .clipboard_watch_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(identity);
        std::thread::spawn(move || {
            let status = child.wait();
            let mut watcher = app
                .inner
                .clipboard_watch_kill
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if watcher
                .as_ref()
                .is_some_and(|identity| identity.incarnation == incarnation)
            {
                *watcher = None;
                if app.inner.clipboard_listener_epoch.load(Ordering::Acquire) == epoch
                    && app.inner.clipboard_listening.swap(false, Ordering::AcqRel)
                {
                    app.refresh_tray();
                    app.set_notice(
                        format!(
                            "Clipboard listener stopped{}",
                            status
                                .ok()
                                .filter(|status| !status.success())
                                .map(|_| " after a helper error")
                                .unwrap_or_default()
                        ),
                        Duration::from_secs(4),
                    );
                    app.show_hud();
                }
            }
        });
        return Ok(());
    }
    std::thread::spawn(move || {
        if let Err(error) = x11_clipboard_listener_loop(&app, epoch) {
            if app.inner.clipboard_listener_epoch.load(Ordering::Acquire) == epoch
                && app.inner.clipboard_listening.swap(false, Ordering::AcqRel)
            {
                app.refresh_tray();
                app.set_notice(
                    format!("Clipboard listener stopped: {error}"),
                    Duration::from_secs(4),
                );
                app.show_hud();
            }
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn x11_clipboard_listener_loop(app: &App, epoch: u64) -> Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{self, SelectionEventMask};
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    let (connection, screen_index) = x11rb::connect(None).context("connecting to X11 clipboard")?;
    xfixes::query_version(&connection, 5, 0)?.reply()?;
    let screen = &connection.setup().roots[screen_index];
    let window = connection.generate_id()?;
    connection.create_window(
        0,
        window,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new(),
    )?;
    let clipboard = connection.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
    xfixes::select_selection_input(
        &connection,
        window,
        clipboard,
        SelectionEventMask::SET_SELECTION_OWNER,
    )?;
    connection.flush()?;
    while app.inner.clipboard_listening.load(Ordering::Acquire)
        && app.inner.clipboard_listener_epoch.load(Ordering::Acquire) == epoch
    {
        if let Some(event) = connection.poll_for_event()? {
            if let x11rb::protocol::Event::XfixesSelectionNotify(event) = event {
                if event.selection == clipboard
                    && event.subtype == x11rb::protocol::xfixes::SelectionEvent::SET_SELECTION_OWNER
                {
                    app.request_clipboard_scan();
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = connection.destroy_window(window);
    let _ = connection.flush();
    Ok(())
}

#[cfg(windows)]
fn start_clipboard_listener(app: App, epoch: u64) -> Result<()> {
    std::thread::spawn(move || {
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
        let mut sequence = unsafe { GetClipboardSequenceNumber() };
        while app.inner.clipboard_listening.load(Ordering::Acquire)
            && app.inner.clipboard_listener_epoch.load(Ordering::Acquire) == epoch
        {
            std::thread::sleep(Duration::from_millis(150));
            let next = unsafe { GetClipboardSequenceNumber() };
            if next != sequence {
                sequence = next;
                app.request_clipboard_scan();
            }
        }
    });
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn start_clipboard_listener(_app: App, _epoch: u64) -> Result<()> {
    bail!("clipboard listening is not available on this platform")
}
struct ServerJob {
    id: String,
    generation: u64,
    owner: String,
    state: Mutex<String>,
    events: Mutex<Vec<serde_json::Value>>,
    manifest: Mutex<Option<serde_json::Value>>,
    audio: Mutex<Option<(Vec<u8>, String)>>,
    created: Instant,
    lifetime: request_lifetime::RequestLifetime,
}
struct RequestFinishGuard {
    lifetime: request_lifetime::RequestLifetime,
    transferred: bool,
}
impl Drop for RequestFinishGuard {
    fn drop(&mut self) {
        if !self.transferred {
            self.lifetime.finish();
        }
    }
}
const MAX_SERVER_JOBS: usize = 32;
const MAX_ACTIVE_SERVER_JOBS: usize = 8;
const MAX_SERVER_JOB_AUDIO: usize = 16 * 1024 * 1024;
const MAX_SERVER_AUDIO_STORAGE: usize = 64 * 1024 * 1024;

struct WorkerSupervisor {
    worker: Mutex<Option<WorkerHandle>>,
    active_request: Mutex<Option<(String, String)>>,
    request_watchers: Arc<Mutex<HashSet<String>>>,
    kill: Mutex<Option<ChildKill>>,
    cancel_writer: Mutex<Option<CancelWriter>>,
    speculation: Mutex<Option<(u64, String, String)>>,
    idle_workers: Arc<(Mutex<Vec<IdleWorker>>, Condvar, AtomicBool)>,
    retiring: Arc<AtomicBool>,
    published_device: Arc<RwLock<Option<String>>>,
    model_lifecycle: Arc<RwLock<ModelLifecycle>>,
    events: model_events::ModelEvents,
    residency: RwLock<Option<reader_core::residency::Estimate>>,
    desktop_priority_waiters: AtomicUsize,
}
struct DesktopTtsPriority<'a>(&'a AtomicUsize);
impl Drop for DesktopTtsPriority<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct OwnedWorkerRequest<'a> {
    supervisor: &'a WorkerSupervisor,
    request_id: String,
    incarnation: String,
}
impl Drop for OwnedWorkerRequest<'_> {
    fn drop(&mut self) {
        let mut active = self
            .supervisor
            .active_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_ref().is_some_and(|(id, incarnation)| {
            id == &self.request_id && incarnation == &self.incarnation
        }) {
            *active = None;
        }
    }
}
struct IdleWorker {
    worker: WorkerHandle,
    kill: ChildKill,
    deadline: Instant,
}

fn terminate_idle(mut idle: IdleWorker) {
    if let Err(error) = terminate_child(&idle.kill) {
        tracing::warn!(%error, "could not terminate cached speech worker");
        // Avoid waiting forever on an unsignalled child.
        let _ = idle.worker.child.kill();
    }
    let _ = idle.worker.child.wait();
}

#[derive(Debug, Clone)]
struct ModelLifecycle {
    status: String,
    revision: u64,
}
impl ModelLifecycle {
    fn transition(&mut self, status: &str) {
        if self.status != status {
            self.status = status.to_owned();
            self.revision = self.revision.saturating_add(1);
        }
    }
}
struct WorkerHandle {
    child: Child,
    stdin: ChildStdin,
    cancel_stdin: File,
    stdin_write_gate: Arc<Mutex<()>>,
    stdout: BufReader<ChildStdout>,
    fingerprint: String,
    keep_warm_seconds: u16,
    last_used: Instant,
    device: String,
    incarnation: String,
    retiring: Arc<AtomicBool>,
    model_lifecycle: Arc<RwLock<ModelLifecycle>>,
    events: model_events::ModelEvents,
    model_id: String,
    runtime_id: String,
    language: String,
    native: bool,
    loading: bool,
    loaded: bool,
}

struct CancelWriter {
    incarnation: String,
    stdin: File,
    write_gate: Arc<Mutex<()>>,
}
impl WorkerHandle {
    fn write_request(&mut self, request: &WorkerRequest) -> Result<()> {
        let _gate = self
            .stdin_write_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.native
            && !self.loading
            && matches!(
                request,
                WorkerRequest::Prepare { .. } | WorkerRequest::Synthesize { .. }
            )
        {
            self.events.record(
                "tts",
                &self.model_id,
                &self.runtime_id,
                &self.language,
                "loading",
            );
            self.loading = true;
        }
        write_json_frame(&mut self.stdin, request)
    }
    fn mark_loaded(&mut self) {
        if self.native && !self.loaded {
            self.loaded = true;
            self.events.record(
                "tts",
                &self.model_id,
                &self.runtime_id,
                &self.language,
                "loaded",
            );
        }
    }
}
impl Drop for WorkerHandle {
    fn drop(&mut self) {
        if self.native && self.loading && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.events.record(
                "tts",
                &self.model_id,
                &self.runtime_id,
                &self.language,
                if self.loaded {
                    "unloaded"
                } else {
                    "loading_canceled"
                },
            );
        }
    }
}

#[cfg(unix)]
fn duplicate_child_stdin(stdin: &ChildStdin) -> Result<File> {
    use std::os::fd::AsFd;
    Ok(File::from(stdin.as_fd().try_clone_to_owned()?))
}
#[cfg(windows)]
fn duplicate_child_stdin(stdin: &ChildStdin) -> Result<File> {
    use std::os::windows::io::AsHandle;
    Ok(File::from(stdin.as_handle().try_clone_to_owned()?))
}
#[cfg(target_os = "linux")]
struct ChildKill {
    incarnation: String,
    pidfd: std::os::fd::OwnedFd,
}
#[cfg(windows)]
struct ChildKill {
    incarnation: String,
    process: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg(windows)]
unsafe impl Send for ChildKill {}
#[cfg(not(any(windows, target_os = "linux")))]
struct ChildKill {
    incarnation: String,
}

fn speculation_mismatch(marker: &(u64, String, String), generation: u64, expected: &str) -> bool {
    marker.0 == generation && marker.2 != expected
}

#[cfg(test)]
mod speculation_tests {
    #[test]
    fn mismatch_only_targets_the_owning_generation() {
        let marker = (7, "pid-identity".to_owned(), "model-a".to_owned());
        assert!(super::speculation_mismatch(&marker, 7, "model-b"));
        assert!(!super::speculation_mismatch(&marker, 7, "model-a"));
        assert!(!super::speculation_mismatch(&marker, 8, "model-b"));
    }
}

impl WorkerSupervisor {
    fn new(events: model_events::ModelEvents) -> Self {
        let idle_workers = Arc::new((
            Mutex::new(Vec::<IdleWorker>::new()),
            Condvar::new(),
            AtomicBool::new(false),
        ));
        let weak = Arc::downgrade(&idle_workers);
        std::thread::spawn(move || {
            let Some(cache) = weak.upgrade() else { return };
            let (lock, changed, closed) = &*cache;
            let mut entries = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                if closed.load(Ordering::Acquire) {
                    break;
                }
                let now = Instant::now();
                if let Some(index) = entries.iter().position(|entry| entry.deadline <= now) {
                    let entry = entries.remove(index);
                    drop(entries);
                    terminate_idle(entry);
                    entries = lock
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    continue;
                }
                entries = if let Some(deadline) = entries.iter().map(|entry| entry.deadline).min() {
                    changed
                        .wait_timeout(entries, deadline.saturating_duration_since(now))
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0
                } else {
                    changed
                        .wait(entries)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                };
            }
        });
        Self {
            worker: Mutex::new(None),
            active_request: Mutex::new(None),
            request_watchers: Arc::new(Mutex::new(HashSet::new())),
            kill: Mutex::new(None),
            cancel_writer: Mutex::new(None),
            speculation: Mutex::new(None),
            events,
            residency: RwLock::new(None),
            idle_workers,
            retiring: Arc::new(AtomicBool::new(false)),
            published_device: Arc::new(RwLock::new(None)),
            desktop_priority_waiters: AtomicUsize::new(0),
            model_lifecycle: Arc::new(RwLock::new(ModelLifecycle {
                status: "Unloaded".into(),
                revision: 0,
            })),
        }
    }

    fn own_request(&self, request_id: &str, incarnation: &str) -> OwnedWorkerRequest<'_> {
        *self
            .active_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((request_id.to_owned(), incarnation.to_owned()));
        OwnedWorkerRequest {
            supervisor: self,
            request_id: request_id.to_owned(),
            incarnation: incarnation.to_owned(),
        }
    }

    fn terminate_if_request_owner(
        &self,
        request_id: &str,
        expected_incarnation: &str,
    ) -> Result<bool> {
        let active = self
            .active_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((owner, incarnation)) = active.as_ref() else {
            return Ok(false);
        };
        if owner != request_id || incarnation != expected_incarnation {
            return Ok(false);
        }
        self.terminate_if_incarnation(incarnation)
    }
    fn start_request_watcher(
        self: &Arc<Self>,
        request_id: &str,
        lifetime: &request_lifetime::RequestLifetime,
        incarnation: &str,
    ) {
        let inserted = self
            .request_watchers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(request_id.to_owned());
        if !inserted {
            return;
        }
        let supervisor = Arc::clone(self);
        let watchers = Arc::clone(&self.request_watchers);
        let lifetime = lifetime.clone();
        let request_id = request_id.to_owned();
        let incarnation = incarnation.to_owned();
        std::thread::spawn(move || {
            let hard_deadline = lifetime.hard_deadline();
            loop {
                if lifetime.stage() == reader_core::request_budget::RequestStage::Finished {
                    break;
                }
                if Instant::now() >= hard_deadline || lifetime.check().is_err() {
                    let _ = supervisor.terminate_if_request_owner(&request_id, &incarnation);
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            watchers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&request_id);
        });
    }
    // Called only with the active worker mutex held. Cancellation always targets
    // the active kill handle; cache entries are never visible to that path.
    fn select_worker(&self, slot: &mut Option<WorkerHandle>, config: &AppConfig) -> Result<()> {
        // A settings change applies to cached models too. Their lifetime starts
        // at completed work, rather than extending each time another model wins.
        let (lock, changed, _) = &*self.idle_workers;
        let mut entries = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in entries.iter_mut() {
            entry.deadline = entry.worker.last_used
                + Duration::from_secs(u64::from(config.playback.keep_warm_seconds));
        }
        changed.notify_one();
        drop(entries);
        let expected = fingerprint(config);
        if slot
            .as_ref()
            .is_some_and(|worker| worker.fingerprint == expected)
        {
            let worker = slot.as_mut().unwrap();
            worker.keep_warm_seconds = config.playback.keep_warm_seconds;
            worker.language.clone_from(&config.speech.language);
            return Ok(());
        }
        let (lock, changed, _) = &*self.idle_workers;
        let mut cache = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let restored = cache
            .iter()
            .position(|entry| {
                entry.worker.fingerprint == expected && entry.deadline > Instant::now()
            })
            .map(|index| cache.remove(index));
        let mut evicted = Vec::new();
        for index in (0..cache.len()).rev() {
            if cache[index].deadline <= Instant::now() {
                evicted.push(cache.remove(index));
            }
        }
        if let Some(current) = slot.take() {
            let identity = self
                .kill
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            *self
                .cancel_writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            if let Some(identity) = identity {
                if identity.incarnation != current.incarnation {
                    *slot = Some(current);
                    *self
                        .kill
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(identity);
                    if let Some(entry) = restored {
                        cache.push(entry);
                    }
                    cache.append(&mut evicted);
                    bail!("speech worker identity mismatch during cache switch");
                }
                let deadline =
                    current.last_used + Duration::from_secs(current.keep_warm_seconds as u64);
                let entry = IdleWorker {
                    worker: current,
                    kill: identity,
                    deadline,
                };
                if entry.deadline > Instant::now() {
                    cache.push(entry);
                } else {
                    evicted.push(entry);
                }
            } else {
                let mut old = Some(current);
                discard_worker(&mut old);
            }
        }
        changed.notify_one();
        drop(cache);
        for entry in evicted {
            terminate_idle(entry);
        }
        if let Some(mut entry) = restored {
            if matches!(entry.worker.child.try_wait(), Ok(None)) {
                self.retiring.store(false, Ordering::Release);
                if let Err(error) = self.install_cancel_writer(&entry.worker) {
                    terminate_idle(entry);
                    return Err(error);
                }
                *self
                    .kill
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(entry.kill);
                entry.worker.keep_warm_seconds = config.playback.keep_warm_seconds;
                entry.worker.language.clone_from(&config.speech.language);
                *slot = Some(entry.worker);
            } else {
                let _ = entry.worker.child.wait();
            }
        }
        Ok(())
    }
    fn clear_idle_workers(&self) {
        let (lock, changed, _) = &*self.idle_workers;
        let entries = std::mem::take(
            &mut *lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        changed.notify_one();
        for entry in entries {
            terminate_idle(entry);
        }
    }
    fn set_model_status(&self, status: &str) {
        self.model_lifecycle
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .transition(status);
    }
    fn mark_model_loading(&self) {
        let mut lifecycle = self
            .model_lifecycle
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lifecycle.transition("Loading");
    }
    fn model_lifecycle(&self) -> ModelLifecycle {
        self.model_lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn set_published_device(&self, device: Option<String>) {
        *self
            .published_device
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = device;
    }
    fn published_device(&self) -> Option<String> {
        self.published_device
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn incarnation(&self) -> Option<String> {
        self.kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|identity| identity.incarnation.clone())
    }
    fn idle_incarnation(&self) -> Option<String> {
        let worker = match self.worker.try_lock() {
            Ok(worker) => worker,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        worker
            .as_ref()
            .filter(|current| !current.retiring.load(Ordering::Acquire))
            .map(|current| current.incarnation.clone())
    }
    #[cfg(target_os = "linux")]
    fn try_incarnation(&self) -> Option<String> {
        let identity = match self.kill.try_lock() {
            Ok(identity) => identity,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        identity
            .as_ref()
            .map(|identity| identity.incarnation.clone())
    }
    fn retire_if(
        &self,
        expected_incarnation: Option<&str>,
        eligible: impl FnOnce() -> bool,
    ) -> Result<bool> {
        // Holding the worker guard across the predicate and retirement prevents
        // delayed cleanup from crossing into a later worker incarnation.
        let mut worker = match self.worker.try_lock() {
            Ok(worker) => worker,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
        };
        let Some(current) = worker.as_ref() else {
            return Ok(false);
        };
        if expected_incarnation.is_some_and(|expected| current.incarnation != expected)
            || !eligible()
        {
            return Ok(false);
        }
        let mut killer = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(identity) = killer.as_ref() {
            if identity.incarnation != current.incarnation {
                return Ok(false);
            }
            current.retiring.store(true, Ordering::Release);
            terminate_child(identity)?;
            *killer = None;
        } else if current.retiring.load(Ordering::Acquire) {
            // An explicit cancellation already signalled this exact child.
            // Finish reaping it while the worker guard still excludes reuse.
        } else {
            return Ok(false);
        }
        self.set_published_device(None);
        if let Some(mut retired) = worker.take() {
            let _ = retired.child.wait();
            retired
                .model_lifecycle
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .transition("Unloaded");
        }
        Ok(true)
    }
    fn terminate_if_incarnation(&self, expected: &str) -> Result<bool> {
        let mut killer = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(identity) = killer.as_ref() else {
            return Ok(false);
        };
        if identity.incarnation != expected {
            return Ok(false);
        }
        self.retiring.store(true, Ordering::Release);
        terminate_child(identity)?;
        *killer = None;
        self.set_published_device(None);
        Ok(true)
    }
    fn kill_current(&self) -> Result<()> {
        self.set_published_device(None);
        let mut killer = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(identity) = killer.as_ref() {
            self.retiring.store(true, Ordering::Release);
            #[cfg(target_os = "linux")]
            unsafe {
                let result = libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    identity.pidfd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                );
                if result < 0 && *libc::__errno_location() != libc::ESRCH {
                    bail!("could not terminate speech worker through pidfd")
                }
            }
            #[cfg(windows)]
            unsafe {
                if windows_sys::Win32::System::Threading::TerminateProcess(identity.process, 1) == 0
                {
                    bail!("could not terminate speech worker process")
                }
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                bail!("safe worker termination is unsupported on this OS")
            }
        }
        *killer = None;
        Ok(())
    }
    fn shutdown(&self) -> Result<()> {
        self.clear_idle_workers();
        self.kill_current()?;
        if let Some(mut worker) = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = worker.child.wait();
            worker
                .model_lifecycle
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .transition("Unloaded");
        }
        Ok(())
    }
    fn reap_retiring(&self) -> bool {
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if worker
            .as_ref()
            .is_some_and(|current| current.retiring.load(Ordering::Acquire))
        {
            discard_worker(&mut worker);
            self.set_published_device(None);
            true
        } else {
            false
        }
    }

    fn cancel_job(
        &self,
        job_id: Option<&str>,
        generation: u64,
        force_terminate: bool,
    ) -> Result<bool> {
        let mut worker = match self.worker.try_lock() {
            Ok(worker) => worker,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                if !force_terminate {
                    let Some(job_id) = job_id else {
                        return Ok(false);
                    };
                    let mut cancel = self
                        .cancel_writer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(cancel) = cancel.as_mut() {
                        let current_incarnation = self
                            .kill
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .as_ref()
                            .map(|identity| identity.incarnation.clone());
                        if current_incarnation.as_deref() != Some(cancel.incarnation.as_str()) {
                            return Ok(false);
                        }
                        let _gate = cancel
                            .write_gate
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        write_json_frame(
                            &mut cancel.stdin,
                            &WorkerRequest::Cancel {
                                job_id: job_id.to_owned(),
                                generation,
                            },
                        )?;
                        return Ok(false);
                    }
                    return Ok(false);
                }
                self.kill_current()?;
                return Ok(true);
            }
        };
        if worker
            .as_ref()
            .is_some_and(|current| current.retiring.load(Ordering::Acquire))
        {
            discard_worker(&mut worker);
            self.set_published_device(None);
            return Ok(true);
        }
        if !worker_alive(&mut worker) {
            self.set_published_device(None);
        }
        Ok(false)
    }

    fn install_cancel_writer(&self, worker: &WorkerHandle) -> Result<()> {
        *self
            .cancel_writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CancelWriter {
            incarnation: worker.incarnation.clone(),
            stdin: worker
                .cancel_stdin
                .try_clone()
                .context("duplicating worker cancellation pipe")?,
            write_gate: Arc::clone(&worker.stdin_write_gate),
        });
        Ok(())
    }
    // Caller holds the intent gate: a stale guess cannot claim a later job.
    fn resolve_speculation(&self, generation: u64, config: &AppConfig) -> Result<()> {
        let mut marker = self
            .speculation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if marker
            .as_ref()
            .is_some_and(|(owner, _, _)| *owner == generation)
        {
            let terminate =
                speculation_mismatch(marker.as_ref().unwrap(), generation, &fingerprint(config));
            let (_, incarnation, _) = marker.take().unwrap();
            if terminate {
                self.terminate_if_incarnation(&incarnation)?;
            }
        }
        Ok(())
    }
    fn prepare(&self, config: &AppConfig) -> Result<()> {
        self.prepare_with_cancel(config, None, None, || true)
    }
    fn prepare_with_cancel(
        &self,
        config: &AppConfig,
        cancel_gate: Option<&Mutex<()>>,
        speculative_generation: Option<u64>,
        current: impl Fn() -> bool,
    ) -> Result<()> {
        let mut guard = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let intent = cancel_gate.map(|gate| {
            gate.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        });
        if !current() {
            bail!("speech preparation was canceled");
        }
        if guard
            .as_ref()
            .is_some_and(|w| w.retiring.load(Ordering::Acquire))
        {
            discard_worker(&mut guard)
        }
        self.select_worker(&mut guard, config)?;
        let cold = !worker_alive(&mut guard);
        if cold {
            self.spawn_into(&mut guard, config)?
        }
        if matches!(
            config.speech.provider,
            SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
        ) {
            if cold {
                self.mark_model_loading();
            } else {
                self.set_model_status("Loaded");
            }
        } else {
            self.set_model_status("Remote service");
        }
        let worker = guard.as_mut().unwrap();
        if let Some(generation) = speculative_generation {
            *self
                .speculation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
                generation,
                worker.incarnation.clone(),
                worker.fingerprint.clone(),
            ));
        }
        worker.write_request(&WorkerRequest::Prepare {
            device: worker.device.clone(),
        })?;
        drop(intent);
        let prepared = loop {
            match read_worker_frame::<WorkerEvent>(&mut worker.stdout) {
                Ok(WorkerFrame::Control(WorkerEvent::Ready { device, .. })) => break Ok(device),
                Ok(WorkerFrame::Control(WorkerEvent::Error { message, .. })) => {
                    break Err(anyhow::anyhow!("speech preparation failed: {message}"));
                }
                Ok(WorkerFrame::Pcm { .. }) => {
                    break Err(anyhow::anyhow!("unexpected PCM during model preparation"));
                }
                Ok(_) => {}
                Err(error) => break Err(error),
            }
        };
        match prepared {
            Ok(device) => {
                guard.as_mut().unwrap().mark_loaded();
                self.set_published_device(Some(device.clone()));
                self.set_model_status(
                    if matches!(
                        config.speech.provider,
                        SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
                    ) {
                        "Loaded"
                    } else {
                        "Remote service"
                    },
                );
                write_device_report(&device, device.contains("CUDA verified"));
                Ok(())
            }
            Err(error) if config.speech.device == DevicePreference::Auto => {
                let intent = cancel_gate.map(|gate| {
                    gate.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                });
                if !current() || self.retiring.load(Ordering::Acquire) {
                    self.kill_current()?;
                    discard_worker(&mut guard);
                    bail!("speech preparation was canceled");
                }
                tracing::warn!(%error,"automatic CUDA worker failed during startup; restarting model on CPU");
                self.kill_current()?;
                discard_worker(&mut guard);
                self.spawn_fallback_into(&mut guard, config)?;
                if !current()
                    || guard
                        .as_ref()
                        .is_some_and(|worker| worker.retiring.load(Ordering::Acquire))
                {
                    self.kill_current()?;
                    discard_worker(&mut guard);
                    bail!("speech preparation was canceled");
                }
                let worker = guard.as_mut().unwrap();
                if let Some(generation) = speculative_generation {
                    *self
                        .speculation
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
                        generation,
                        worker.incarnation.clone(),
                        worker.fingerprint.clone(),
                    ));
                }
                worker.write_request(&WorkerRequest::Prepare {
                    device: "cpu".into(),
                })?;
                drop(intent);
                match read_worker_frame::<WorkerEvent>(&mut worker.stdout)? {
                    WorkerFrame::Control(WorkerEvent::Ready { device, .. }) => {
                        guard.as_mut().unwrap().mark_loaded();
                        self.set_published_device(Some(device.clone()));
                        self.set_model_status("Loaded");
                        write_device_report("CPU · CUDA worker startup failed", false);
                        Ok(())
                    }
                    _ => bail!("CPU voice preparation failed after CUDA startup failure"),
                }
            }
            Err(error) => {
                // A worker may have emitted an error while its process is still alive.
                // Keep the lifecycle state until that exact child is actually reaped.
                Err(error)
            }
        }
    }
    fn collect(
        self: &Arc<Self>,
        config: &AppConfig,
        text: &str,
        options: SynthesisOptions<'_>,
    ) -> Result<(Vec<i16>, u32)> {
        let SynthesisOptions {
            priority,
            starts_in_code,
            voice,
            speed,
            request_id,
            lifetime,
            current,
        } = options;
        let _priority = priority.then(|| {
            self.desktop_priority_waiters.fetch_add(1, Ordering::AcqRel);
            DesktopTtsPriority(&self.desktop_priority_waiters)
        });
        let units = plan_units(text, 240)?;
        let mut result = Vec::new();
        let mut rate = 24_000u32;
        let generation = 1;
        let job = request_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        for unit in units {
            anyhow::ensure!(current(), "server job was cancelled");
            if let Some(lifetime) = lifetime {
                lifetime.check()?;
            }
            // API synthesis yields at each unit boundary. Desktop playback
            // gets a bounded first window to take the shared worker.
            let priority_deadline = Instant::now() + Duration::from_millis(350);
            while !priority
                && self.desktop_priority_waiters.load(Ordering::Acquire) > 0
                && Instant::now() < priority_deadline
                && current()
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut guard = loop {
                if let Some(lifetime) = lifetime {
                    lifetime.check()?;
                }
                match self.worker.try_lock() {
                    Ok(guard) => break guard,
                    Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            };
            if guard
                .as_ref()
                .is_some_and(|w| w.retiring.load(Ordering::Acquire))
            {
                discard_worker(&mut guard)
            }
            let mut cpu_fallback = false;
            self.select_worker(&mut guard, config)?;
            let cold_start = !worker_alive(&mut guard);
            if let Some(lifetime) = lifetime {
                if matches!(
                    lifetime.stage(),
                    reader_core::request_budget::RequestStage::Connecting
                        | reader_core::request_budget::RequestStage::Queued
                ) {
                    lifetime.set_stage(if cold_start {
                        reader_core::request_budget::RequestStage::LoadingModel
                    } else {
                        reader_core::request_budget::RequestStage::Inference
                    });
                }
            }
            'retry: loop {
                if !worker_alive(&mut guard) {
                    if cpu_fallback {
                        self.spawn_fallback_into(&mut guard, config)?
                    } else {
                        self.spawn_into(&mut guard, config)?
                    }
                }
                anyhow::ensure!(current(), "server job was cancelled");
                let worker = guard.as_mut().unwrap();
                let _active_request =
                    request_id.map(|id| self.own_request(id, &worker.incarnation));
                if let (Some(request_id), Some(lifetime)) = (request_id, lifetime) {
                    self.start_request_watcher(request_id, lifetime, &worker.incarnation);
                }
                if let Some(lifetime) = lifetime {
                    lifetime.check()?;
                }
                worker.write_request(&WorkerRequest::Synthesize {
                    job_id: job.clone(),
                    generation,
                    sentence: unit.sentence_index as u32,
                    unit: unit.unit_index as u32,
                    text: prepare_speech_text(
                        config,
                        &unit.text,
                        unit.starts_in_code ^ starts_in_code,
                    ),
                    voice: voice.into(),
                    steps: config.speech.selection.single.steps,
                    language: config.speech.language.clone(),
                    speed,
                    device: worker.device.clone(),
                })?;
                let mut offset = 0u64;
                let mut saw_final = false;
                loop {
                    let frame = match read_worker_frame::<WorkerEvent>(&mut worker.stdout) {
                        Ok(frame) => frame,
                        Err(error)
                            if config.speech.device == DevicePreference::Auto && !cpu_fallback =>
                        {
                            anyhow::ensure!(current(), "server job was cancelled");
                            tracing::warn!(%error,"automatic CUDA server worker exited; retrying current utterance on CPU");
                            if let Some(lifetime) = lifetime {
                                if let Some(delay) =
                                    request_lifetime::RetryPolicy::default_network()
                                        .delay_before_retry(1, lifetime)
                                {
                                    std::thread::sleep(delay);
                                    lifetime.check()?;
                                }
                            }
                            if let (Some(request_id), Some(worker)) = (request_id, guard.as_ref()) {
                                self.terminate_if_request_owner(request_id, &worker.incarnation)?;
                            } else {
                                self.kill_current()?;
                            }
                            discard_worker(&mut guard);
                            self.spawn_fallback_into(&mut guard, config)?;
                            cpu_fallback = true;
                            continue 'retry;
                        }
                        Err(error) => return Err(error),
                    };
                    match frame {
                        WorkerFrame::Pcm { header, samples } => {
                            anyhow::ensure!(
                                header.job_id == job
                                    && header.generation == generation
                                    && header.sentence == unit.sentence_index as u32
                                    && header.unit == unit.unit_index as u32
                                    && header.sample_offset == offset
                                    && header.sample_count as usize == samples.len()
                                    && !saw_final,
                                "speech worker returned an unexpected PCM timeline"
                            );
                            rate = header.sample_rate;
                            offset += samples.len() as u64;
                            saw_final = header.final_chunk;
                            result.extend(samples);
                            anyhow::ensure!(
                                result.len() <= rate as usize * 300,
                                "server audio exceeds the five-minute output cap"
                            );
                        }
                        WorkerFrame::Control(WorkerEvent::Ready { device, .. }) => {
                            worker.mark_loaded();
                            if let Some(lifetime) = lifetime.filter(|lifetime| {
                                lifetime.stage()
                                    == reader_core::request_budget::RequestStage::LoadingModel
                            }) {
                                lifetime.set_stage(
                                    reader_core::request_budget::RequestStage::Inference,
                                );
                            }
                            self.set_published_device(Some(device.clone()));
                            write_device_report(&device, device.contains("CUDA verified"))
                        }
                        WorkerFrame::Control(WorkerEvent::Finished {
                            job_id,
                            generation: g,
                        }) if job_id == job && g == generation => {
                            worker.last_used = Instant::now();
                            anyhow::ensure!(
                                saw_final,
                                "speech worker finished without a final PCM packet"
                            );
                            break 'retry;
                        }
                        WorkerFrame::Control(WorkerEvent::Error {
                            job_id: Some(job_id),
                            generation: Some(g),
                            message,
                            ..
                        }) if job_id == job && g == generation => {
                            bail!("speech worker failed: {message}")
                        }
                        _ => {}
                    }
                    if let Some(lifetime) = lifetime {
                        lifetime.check()?;
                    }
                }
            }
        }
        Ok((result, rate))
    }
    fn synthesize(
        &self,
        app: &App,
        config: &AppConfig,
        session: &Session,
        unit: &SpeechUnit,
    ) -> Result<()> {
        if !app.inner.generation.accepts(session.generation) {
            return Ok(());
        }
        let mut guard = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard
            .as_ref()
            .is_some_and(|w| w.retiring.load(Ordering::Acquire))
        {
            discard_worker(&mut guard)
        }
        let sentence = unit.sentence_index as u32;
        let unit_id = unit.unit_index as u32;
        let mut cpu_fallback = false;
        'retry: loop {
            let intent = app
                .inner
                .intent_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !app.inner.generation.accepts(session.generation) {
                return Ok(());
            }
            self.select_worker(&mut guard, config)?;
            let cold = !worker_alive(&mut guard);
            if cold {
                if cpu_fallback {
                    self.spawn_fallback_into(&mut guard, config)?
                } else {
                    self.spawn_into(&mut guard, config)?
                }
            }
            if !app.inner.generation.accepts(session.generation) {
                return Ok(());
            }
            if matches!(
                config.speech.provider,
                SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
            ) {
                if cold {
                    self.mark_model_loading();
                    let mut activity = app.inner.activity.lock().unwrap();
                    if activity.generation == session.generation {
                        activity.state = "loading";
                        activity.set(session.generation, "loading_voice");
                    }
                } else {
                    self.set_model_status("Loaded");
                    let mut activity = app.inner.activity.lock().unwrap();
                    if activity.generation == session.generation {
                        activity.state = "ready";
                    }
                }
            } else {
                self.set_model_status("Remote service");
            }
            {
                let worker = guard.as_mut().unwrap();
                let prepared_text = prepare_speech_text(config, &unit.text, unit.starts_in_code);
                worker.write_request(&WorkerRequest::Synthesize {
                    job_id: session.job_id.clone(),
                    generation: session.generation,
                    sentence,
                    unit: unit_id,
                    text: prepared_text.clone(),
                    voice: config.speech.voice.clone(),
                    steps: config.speech.selection.single.steps,
                    language: config.speech.language.clone(),
                    speed: config.speech.speed,
                    device: worker.device.clone(),
                })?;
                app.inner
                    .activity
                    .lock()
                    .unwrap()
                    .set_prepared_text(session.generation, prepared_text);
            }
            drop(intent);
            let mut sample_offset = 0u64;
            let mut saw_final = false;
            loop {
                let frame = {
                    let worker = guard.as_mut().unwrap();
                    read_worker_frame::<WorkerEvent>(&mut worker.stdout)
                };
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(_error) if !app.inner.generation.accepts(session.generation) => {
                        return Ok(());
                    }
                    Err(error)
                        if config.speech.device == DevicePreference::Auto && !cpu_fallback =>
                    {
                        tracing::warn!(%error,"automatic CUDA worker exited; retrying the unsent sentence on a fresh CPU worker");
                        self.kill_current()?;
                        discard_worker(&mut guard);
                        self.spawn_fallback_into(&mut guard, config)?;
                        cpu_fallback = true;
                        write_device_report(
                            "CPU · automatic CUDA worker failed; sentence retried",
                            false,
                        );
                        continue 'retry;
                    }
                    Err(error) => return Err(error),
                };
                match frame {
                    WorkerFrame::Pcm { header, samples } => {
                        if !app.inner.generation.accepts(session.generation) {
                            continue;
                        }
                        anyhow::ensure!(
                            header.job_id == session.job_id
                                && header.generation == session.generation
                                && header.sentence == sentence
                                && header.unit == unit_id
                                && header.sample_offset == sample_offset
                                && header.sample_count as usize == samples.len()
                                && !saw_final,
                            "speech worker returned an unexpected PCM timeline"
                        );
                        sample_offset += samples.len() as u64;
                        saw_final = header.final_chunk;
                        if !app.inner.audio.open(
                            config.playback.output_device.as_deref(),
                            config.playback.max_buffer_seconds,
                            session.generation,
                            &|| app.inner.generation.accepts(session.generation),
                        )? {
                            continue;
                        }
                        let audio =
                            resample(&samples, header.sample_rate, app.inner.audio.output_rate());
                        app.inner.audio.push(
                            AudioFrame {
                                generation: session.generation,
                                sentence,
                                unit: unit_id,
                                samples: &audio,
                            },
                            &config.playback,
                            || app.inner.generation.accepts(session.generation),
                        )?;
                        if !audio.is_empty() && app.inner.generation.accepts(session.generation) {
                            let mut activity = app.inner.activity.lock().unwrap();
                            if activity.generation == session.generation {
                                if activity.phase == "paused" {
                                    activity.before_pause = "reading";
                                } else {
                                    activity.set(session.generation, "reading");
                                }
                            }
                        }
                    }
                    WorkerFrame::Control(WorkerEvent::Ready { device, engine, .. }) => {
                        guard.as_mut().unwrap().mark_loaded();
                        {
                            let mut activity = app.inner.activity.lock().unwrap();
                            if activity.generation == session.generation {
                                activity.state = "ready";
                                if activity.phase == "loading_voice" {
                                    activity.set(session.generation, "synthesizing");
                                }
                            }
                        }
                        app.inner.tts.set_published_device(Some(device.clone()));
                        if matches!(
                            config.speech.provider,
                            SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
                        ) {
                            app.inner.tts.set_model_status("Loaded");
                        } else {
                            app.inner.tts.set_model_status("Remote service");
                        }
                        let proven = device.contains("CUDA verified");
                        write_device_report(&device, proven);
                        tracing::info!(%engine,%device,"speech worker route is ready");
                    }
                    WorkerFrame::Control(WorkerEvent::Finished { job_id, generation })
                        if job_id == session.job_id && generation == session.generation =>
                    {
                        if let Some(worker) = guard.as_mut() {
                            worker.last_used = Instant::now();
                        }
                        if !app.inner.generation.accepts(session.generation) {
                            return Ok(());
                        }
                        anyhow::ensure!(
                            saw_final,
                            "speech worker finished without a final PCM packet"
                        );
                        return Ok(());
                    }
                    WorkerFrame::Control(WorkerEvent::Cancelled { job_id, generation })
                        if job_id == session.job_id && generation == session.generation =>
                    {
                        return Ok(());
                    }
                    WorkerFrame::Control(WorkerEvent::Error {
                        job_id: Some(job_id),
                        generation: Some(generation),
                        message,
                        ..
                    }) if job_id == session.job_id && generation == session.generation => {
                        bail!("speech worker failed: {message}")
                    }
                    _ => {}
                }
            }
        }
    }
    fn spawn_into(&self, slot: &mut Option<WorkerHandle>, config: &AppConfig) -> Result<()> {
        let (worker, kill) = spawn_tts(
            config,
            Arc::clone(&self.retiring),
            Arc::clone(&self.model_lifecycle),
            self.events.clone(),
        )?;
        self.retiring.store(false, Ordering::Release);
        self.install_cancel_writer(&worker)?;
        *self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(kill);
        *slot = Some(worker);
        Ok(())
    }
    fn spawn_fallback_into(
        &self,
        slot: &mut Option<WorkerHandle>,
        config: &AppConfig,
    ) -> Result<()> {
        let mut cpu = config.clone();
        cpu.speech.device = DevicePreference::Cpu;
        let (mut worker, kill) = spawn_tts(
            &cpu,
            Arc::clone(&self.retiring),
            Arc::clone(&self.model_lifecycle),
            self.events.clone(),
        )?;
        self.retiring.store(false, Ordering::Release);
        worker.fingerprint = fingerprint(config);
        self.install_cancel_writer(&worker)?;
        *self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(kill);
        *slot = Some(worker);
        Ok(())
    }
}

impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        self.clear_idle_workers();
        let (lock, changed, closed) = &*self.idle_workers;
        let _guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        closed.store(true, Ordering::Release);
        changed.notify_one();
    }
}

fn terminate_child(identity: &ChildKill) -> Result<()> {
    #[cfg(target_os = "linux")]
    unsafe {
        let result = libc::syscall(
            libc::SYS_pidfd_send_signal,
            identity.pidfd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        );
        if result < 0 && *libc::__errno_location() != libc::ESRCH {
            bail!("could not terminate speech worker through pidfd")
        }
    }
    #[cfg(windows)]
    unsafe {
        if windows_sys::Win32::System::Threading::TerminateProcess(identity.process, 1) == 0 {
            bail!("could not terminate speech worker process")
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    bail!("safe worker termination is unsupported on this OS");
    Ok(())
}

fn paused_release_is_current(inner: &AppInner, generation: u64, lease: u64) -> bool {
    inner.pause_lease_epoch.load(Ordering::Acquire) == lease
        && inner.generation.accepts(generation)
        && inner.paused_restart_generation.load(Ordering::Acquire) == generation
        && audio_is_paused(&inner.audio)
        && inner
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|session| session.generation == generation)
}

fn release_paused_worker(inner: Arc<AppInner>, incarnation: String, generation: u64, lease: u64) {
    for _ in 0..40 {
        match inner.tts.retire_if(Some(&incarnation), || {
            paused_release_is_current(&inner, generation, lease)
        }) {
            Ok(true) => {
                write_device_report("Stopped · long pause released the voice worker", false);
                App {
                    inner: Arc::clone(&inner),
                }
                .show_hud();
                return;
            }
            Ok(false) if paused_release_is_current(&inner, generation, lease) => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(%error,"long-pause worker cleanup failed");
                return;
            }
        }
    }
    // Generation/lease invalidation uses this same short gate. Holding it
    // closes the check-to-kill race without waiting on the worker mutex.
    let lifecycle = inner
        .pause_lifecycle_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !paused_release_is_current(&inner, generation, lease) {
        return;
    }
    let termination = inner.tts.terminate_if_incarnation(&incarnation);
    drop(lifecycle);
    match termination {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error,"could not terminate a stalled paused voice worker");
            return;
        }
    }
    for _ in 0..80 {
        match inner.tts.retire_if(Some(&incarnation), || {
            paused_release_is_current(&inner, generation, lease)
        }) {
            Ok(true) => {
                write_device_report("Stopped · long pause released the voice worker", false);
                App {
                    inner: Arc::clone(&inner),
                }
                .show_hud();
                return;
            }
            Ok(false) if paused_release_is_current(&inner, generation, lease) => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(%error,"long-pause worker reap failed");
                return;
            }
        }
    }
    tracing::warn!("paused speech worker did not release after targeted termination");
}

fn configure_worker_requests(command: &mut Command, config: &AppConfig, fallback: bool) {
    command.arg("--fallback-to-local").arg(fallback.to_string());
    let budgets = config.request_budgets;
    for (name, duration) in [
        ("connect", budgets.connect),
        ("queue", budgets.queue),
        ("cold-model-load", budgets.cold_model_load),
        ("warm-model-load", budgets.warm_model_load),
        ("warm-inference", budgets.warm_inference),
        ("stream-inactivity", budgets.stream_inactivity),
        ("total", budgets.total),
    ] {
        command
            .arg(format!("--request-{name}-timeout-secs"))
            .arg(duration.as_secs().to_string());
    }
}

fn spawn_tts(
    config: &AppConfig,
    retiring: Arc<AtomicBool>,
    model_lifecycle: Arc<RwLock<ModelLifecycle>>,
    events: model_events::ModelEvents,
) -> Result<(WorkerHandle, ChildKill)> {
    let executable = sibling("reader-tts")?;
    let mut command = Command::new(executable);
    configure_worker_requests(&mut command, config, config.speech.fallback_to_local);
    command.args([
        "--fallback-provider",
        if config.speech.selection.single.model.as_deref() == Some("supertonic3")
            || config
                .speech
                .model_dir
                .file_name()
                .is_some_and(|name| name == "supertonic3")
        {
            "local_supertonic"
        } else {
            "local_kokoro"
        },
    ]);
    command
        .arg("--model-dir")
        .arg(&config.speech.model_dir)
        .arg("--runtime-dir")
        .arg(&config.speech.runtime_dir)
        .arg("--device")
        .arg(device_name(config.speech.device))
        .arg("--language")
        .arg(&config.speech.language);
    if matches!(
        config.speech.provider,
        reader_core::SpeechProvider::LocalSupertonic
    ) {
        command.args(["--provider", "local_supertonic"]);
    }
    if let reader_core::SpeechProvider::OpenAiCompatible {
        base_url,
        model,
        voice,
        format,
    } = &config.speech.provider
    {
        command.args([
            "--provider",
            "openai_compatible",
            "--base-url",
            base_url,
            "--model",
            model,
            "--remote-voice",
            voice,
            "--format",
            format,
        ]);
        command.env(
            "SIMPLE_TTS_SPEECH_API_KEY",
            if let Some(slot) = &config.speech.key_slot {
                reader_core::secrets::connection_key(slot)?
            } else if config.speech.use_feature_key {
                reader_core::secrets::speech_key()?
            } else {
                None
            }
            .unwrap_or_default(),
        );
    }
    if let Some(fallback) = &config.speech.remote_fallback {
        command
            .arg("--remote-fallback")
            .arg(serde_json::to_string(fallback)?);
        command.env(
            "SIMPLE_TTS_FALLBACK_API_KEY",
            if let Some(slot) = &fallback.key_slot {
                reader_core::secrets::connection_key(slot)?
            } else {
                None
            }
            .unwrap_or_default(),
        );
    }
    if let Some(library) = &config.speech.worker_library {
        command.arg("--library").arg(library);
    }
    command.env(
        "LD_LIBRARY_PATH",
        reader_core::native_paths::library_search_path(
            &config.speech.runtime_dir,
            std::env::var_os("LD_LIBRARY_PATH").as_deref(),
        )?,
    );
    #[cfg(windows)]
    {
        let mut path = config
            .speech
            .runtime_dir
            .join("bin")
            .to_string_lossy()
            .into_owned();
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(';');
            path.push_str(&existing.to_string_lossy());
        }
        command.env("PATH", path);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().context("starting native speech worker")?;
    let incarnation = WorkerIncarnation::fresh().0.to_string();
    let stdin = child.stdin.take().context("speech worker stdin missing")?;
    let cancel_stdin = duplicate_child_stdin(&stdin)?;
    let stdin_write_gate = Arc::new(Mutex::new(()));
    let stdout = BufReader::new(
        child
            .stdout
            .take()
            .context("speech worker stdout missing")?,
    );
    let kill = ChildKill::new(child.id(), incarnation.clone())?;
    // Hello confirms protocol compatibility; model allocations remain deferred until Prepare or speech.
    let mut worker = WorkerHandle {
        child,
        stdin,
        cancel_stdin,
        stdin_write_gate,
        stdout,
        fingerprint: fingerprint(config),
        keep_warm_seconds: config.playback.keep_warm_seconds,
        last_used: Instant::now(),
        device: device_name(config.speech.device).into(),
        incarnation: incarnation.clone(),
        retiring,
        model_lifecycle,
        events,
        model_id: model_events::identity(&config.speech.model_dir),
        runtime_id: model_events::identity(&config.speech.runtime_dir),
        language: config.speech.language.clone(),
        native: matches!(
            config.speech.provider,
            SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
        ),
        loading: false,
        loaded: false,
    };
    worker.write_request(&WorkerRequest::Hello {
        protocol: 1,
        incarnation: incarnation.clone(),
    })?;
    match read_worker_frame::<WorkerEvent>(&mut worker.stdout)? {
        WorkerFrame::Control(WorkerEvent::Ready { .. }) => {}
        _ => bail!("speech worker handshake failed"),
    }
    Ok((worker, kill))
}

fn discard_worker(worker: &mut Option<WorkerHandle>) {
    if let Some(mut process) = worker.take() {
        let _ = process.child.wait();
        process
            .model_lifecycle
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .transition("Unloaded");
    }
}
fn worker_alive(worker: &mut Option<WorkerHandle>) -> bool {
    let Some(process) = worker.as_mut() else {
        return false;
    };
    match process.child.try_wait() {
        Ok(None) => true,
        _ => {
            if let Some(process) = worker.take() {
                process
                    .model_lifecycle
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .transition("Unloaded");
            }
            false
        }
    }
}

#[cfg(target_os = "linux")]
impl ChildKill {
    fn new(pid: u32, incarnation: String) -> Result<Self> {
        use std::os::fd::FromRawFd;
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) } as i32;
        if fd < 0 {
            bail!("pidfd_open failed; safe speech-worker cancellation is unavailable")
        };
        Ok(Self {
            incarnation,
            pidfd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
        })
    }
    fn terminate(&self, label: &str) -> Result<()> {
        unsafe {
            let result = libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
            if result < 0 && *libc::__errno_location() != libc::ESRCH {
                bail!("could not terminate {label} through pidfd")
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
impl ChildKill {
    fn new(pid: u32, incarnation: String) -> Result<Self> {
        let process = unsafe {
            windows_sys::Win32::System::Threading::OpenProcess(
                windows_sys::Win32::System::Threading::PROCESS_TERMINATE
                    | windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if process.is_null() {
            bail!("could not retain a stable Windows speech-worker process handle")
        };
        Ok(Self {
            incarnation,
            process,
        })
    }
    fn terminate(&self, label: &str) -> Result<()> {
        if unsafe { windows_sys::Win32::System::Threading::TerminateProcess(self.process, 1) } == 0
        {
            bail!("could not terminate {label} process")
        };
        Ok(())
    }
}
#[cfg(not(any(windows, target_os = "linux")))]
impl ChildKill {
    fn new(_pid: u32, incarnation: String) -> Result<Self> {
        Ok(Self { incarnation })
    }
    fn terminate(&self, label: &str) -> Result<()> {
        let _ = (self, label);
        bail!("safe process termination is unsupported on this OS")
    }
}
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;

fn sibling(name: &str) -> Result<PathBuf> {
    let current = std::env::current_exe().context("locating controller executable")?;
    let parent = current
        .parent()
        .context("controller executable has no parent")?;
    #[cfg(test)]
    let parent = if parent.file_name().is_some_and(|name| name == "deps") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    Ok(parent.join(format!("{name}{}", if cfg!(windows) { ".exe" } else { "" })))
}
fn device_name(device: DevicePreference) -> &'static str {
    match device {
        DevicePreference::Auto => "auto",
        DevicePreference::Cpu => "cpu",
        DevicePreference::Cuda => "cuda",
    }
}
fn fingerprint(config: &AppConfig) -> String {
    let credentials = if matches!(
        config.speech.provider,
        SpeechProvider::OpenAiCompatible { .. }
    ) {
        format!(
            "{:?}|{:?}|{}",
            config.speech.key_slot,
            config.speech.remote_fallback,
            reader_core::secrets::credential_revision()
        )
    } else {
        String::new()
    };
    format!(
        "{}|{}|{}|{}|{:?}|{}",
        config.speech.model_dir.display(),
        config.speech.runtime_dir.display(),
        device_name(config.speech.device),
        if matches!(config.speech.provider, SpeechProvider::LocalSupertonic) {
            "multilingual"
        } else {
            &config.speech.language
        },
        config.speech.provider,
        credentials
    )
}

fn prepare_speech_text(config: &AppConfig, text: &str, starts_in_code: bool) -> String {
    let model = if matches!(config.speech.provider, SpeechProvider::LocalKokoro) {
        config
            .speech
            .selection
            .single
            .model
            .as_deref()
            .unwrap_or("")
    } else if matches!(config.speech.provider, SpeechProvider::LocalSupertonic) {
        "supertonic3"
    } else {
        ""
    };
    let spoken = reader_core::text_prep::exclude_words(text, &config.speech.excluded_words);
    reader_core::text_prep::normalize_with_context(
        &spoken,
        model,
        &config.speech.language,
        &config.speech.selection.single,
        starts_in_code,
    )
}

#[derive(Clone)]
struct AudioOutput {
    inner: Arc<AudioInner>,
}
struct AudioFrame<'a> {
    generation: u64,
    sentence: u32,
    unit: u32,
    samples: &'a [i16],
}
struct AudioInner {
    state: Mutex<AudioState>,
    changed: Condvar,
    stream: Mutex<Option<cpal::Stream>>,
    stream_epoch: AtomicU64,
    rate: AtomicU64,
    current_sentence: AtomicUsize,
    current_unit: AtomicUsize,
    callback_buffers: AtomicU64,
    callback_frames: AtomicU64,
    callback_contention: AtomicU64,
    callback_errors: AtomicU64,
    fatal_error: Mutex<Option<String>>,
    fatal_reported: AtomicBool,
}
struct AudioState {
    queue: VecDeque<TaggedSample>,
    max_samples: usize,
    paused: bool,
    generation: u64,
    producer_done: bool,
    stream_open: bool,
}
#[derive(Clone, Copy)]
struct TaggedSample {
    sample: i16,
    sentence: u32,
    unit: u32,
}
fn audio_is_idle(audio: &AudioOutput) -> bool {
    let state = audio
        .inner
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.producer_done && state.queue.is_empty() && !state.paused
}
fn audio_is_paused(audio: &AudioOutput) -> bool {
    audio
        .inner
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .paused
}
impl AudioOutput {
    fn new() -> Self {
        Self {
            inner: Arc::new(AudioInner {
                state: Mutex::new(AudioState {
                    queue: VecDeque::with_capacity(24_000 * 8),
                    max_samples: 24_000 * 8,
                    paused: false,
                    generation: 0,
                    producer_done: true,
                    stream_open: false,
                }),
                changed: Condvar::new(),
                stream: Mutex::new(None),
                stream_epoch: AtomicU64::new(0),
                rate: AtomicU64::new(48_000),
                current_sentence: AtomicUsize::new(0),
                current_unit: AtomicUsize::new(0),
                callback_buffers: AtomicU64::new(0),
                callback_frames: AtomicU64::new(0),
                callback_contention: AtomicU64::new(0),
                callback_errors: AtomicU64::new(0),
                fatal_error: Mutex::new(None),
                fatal_reported: AtomicBool::new(false),
            }),
        }
    }
    fn output_rate(&self) -> u32 {
        self.inner.rate.load(Ordering::Relaxed) as u32
    }
    fn open(
        &self,
        preferred: Option<&str>,
        max_buffer_seconds: u8,
        generation: u64,
        current: &impl Fn() -> bool,
    ) -> Result<bool> {
        let mut stream_lock = self
            .inner
            .stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation != generation || !current() {
            return Ok(false);
        }
        if stream_lock.is_some() {
            return Ok(true);
        }
        state.max_samples = self.inner.rate.load(Ordering::Relaxed) as usize
            * max_buffer_seconds.clamp(1, 8) as usize;
        drop(state);
        *self
            .inner
            .fatal_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.inner.fatal_reported.store(false, Ordering::Relaxed);
        let stream_epoch = self.inner.stream_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let host = cpal::default_host();
        let preferred = preferred.filter(|name| !name.trim().is_empty());
        let device = if let Some(name) = preferred {
            host.output_devices()?
                .find(|device| device.to_string() == name)
                .with_context(|| {
                    format!("configured audio output device '{name}' is unavailable")
                })?
        } else {
            host.default_output_device()
                .context("no default audio output device")?
        };
        let supported = device
            .default_output_config()
            .context("reading default output format")?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        self.inner.rate.store(rate as u64, Ordering::Relaxed);
        let ctx = Arc::clone(&self.inner);
        let channels = config.channels as usize;
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                build_output::<f32>(&device, &config, ctx, channels, stream_epoch, |v| v, |s| s)
            }
            cpal::SampleFormat::I16 => build_output::<i16>(
                &device,
                &config,
                ctx,
                channels,
                stream_epoch,
                |v| v as f32 / 32768.0,
                |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            ),
            cpal::SampleFormat::U16 => build_output::<u16>(
                &device,
                &config,
                ctx,
                channels,
                stream_epoch,
                |v| (v as f32 - 32768.0) / 32768.0,
                |s| ((s.clamp(-1.0, 1.0) + 1.0) * 32767.5) as u16,
            ),
            other => bail!("unsupported output format: {other:?}"),
        }?;
        stream.play().context("starting audio output")?;
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation != generation || !current() {
            return Ok(false);
        }
        state.max_samples = rate as usize * max_buffer_seconds.clamp(1, 8) as usize;
        state.stream_open = true;
        *stream_lock = Some(stream);
        Ok(true)
    }
    fn push(
        &self,
        frame: AudioFrame<'_>,
        playback: &PlaybackSettings,
        current: impl Fn() -> bool,
    ) -> Result<()> {
        if !self.open(
            playback.output_device.as_deref(),
            playback.max_buffer_seconds,
            frame.generation,
            &current,
        )? {
            return Ok(());
        }
        let mut cursor = 0;
        while cursor < frame.samples.len() {
            if let Some(error) = self
                .inner
                .fatal_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                bail!("audio output failed: {error}")
            }
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while state.queue.len() >= state.max_samples
                && state.generation == frame.generation
                && current()
                && self
                    .inner
                    .fatal_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_none()
            {
                state = self
                    .inner
                    .changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
            }
            if let Some(error) = self
                .inner
                .fatal_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                bail!("audio output failed: {error}")
            }
            if state.generation != frame.generation || !current() {
                return Ok(());
            }
            let count = (state.max_samples - state.queue.len())
                .min(2_048)
                .min(frame.samples.len() - cursor);
            for sample in &frame.samples[cursor..cursor + count] {
                state.queue.push_back(TaggedSample {
                    sample: *sample,
                    sentence: frame.sentence,
                    unit: frame.unit,
                });
            }
            cursor += count;
            drop(state);
            self.inner.changed.notify_one();
        }
        Ok(())
    }
    fn replace(&self, generation: u64) {
        let mut s = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generation < s.generation {
            return;
        }
        s.generation = generation;
        s.queue.clear();
        s.paused = false;
        s.producer_done = false;
        self.inner.changed.notify_all();
    }
    fn pause(&self, value: bool, generation: u64) {
        let mut s = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.generation != generation {
            return;
        }
        s.paused = value;
        self.inner.changed.notify_all();
    }
    fn finish(&self, generation: u64) {
        let mut s = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.generation == generation {
            s.producer_done = true;
            self.inner.changed.notify_all();
        }
    }
    fn wait_drained(&self, generation: u64) -> Result<()> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.generation == generation
            && (!state.producer_done || !state.queue.is_empty() || state.paused)
            && self
                .inner
                .fatal_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        {
            state = self
                .inner
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if state.generation == generation {
            drop(state);
            let mut stream = self
                .inner
                .stream
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.generation == generation {
                self.inner.stream_epoch.fetch_add(1, Ordering::AcqRel);
                state.stream_open = false;
                drop(state);
                stream.take();
            }
        }
        if let Some(error) = self
            .inner
            .fatal_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            bail!("audio output failed: {error}")
        }
        Ok(())
    }
    fn stop(&self, generation: u64) {
        let mut stream = self
            .inner
            .stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.generation = generation;
        state.queue.clear();
        state.paused = false;
        state.producer_done = true;
        self.inner.stream_epoch.fetch_add(1, Ordering::AcqRel);
        state.stream_open = false;
        *self
            .inner
            .fatal_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.inner.fatal_reported.store(false, Ordering::Relaxed);
        self.inner.changed.notify_all();
        drop(state);
        stream.take();
    }
    fn reset_cursor(&self) {
        self.inner.current_sentence.store(0, Ordering::Relaxed);
        self.inner.current_unit.store(0, Ordering::Relaxed);
    }
    fn cursor(&self) -> (usize, usize) {
        (
            self.inner.current_sentence.load(Ordering::Relaxed),
            self.inner.current_unit.load(Ordering::Relaxed),
        )
    }
    fn queued(&self) -> usize {
        self.inner.state.lock().unwrap().queue.len()
    }
}

fn build_output<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<AudioInner>,
    channels: usize,
    epoch: u64,
    from_sample: impl Fn(T) -> f32 + Send + Sync + Copy + 'static,
    to_sample: impl Fn(f32) -> T + Send + Sync + Copy + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Copy,
{
    let error_state = Arc::clone(&state);
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                if state.stream_epoch.load(Ordering::Acquire) != epoch {
                    for sample in data.iter_mut() {
                        *sample = to_sample(0.0)
                    }
                    return;
                }
                if let Ok(mut queue) = state.state.try_lock() {
                    if !queue.stream_open || state.stream_epoch.load(Ordering::Acquire) != epoch {
                        for sample in data.iter_mut() {
                            *sample = to_sample(0.0)
                        }
                        return;
                    }
                    state.callback_buffers.fetch_add(1, Ordering::Relaxed);
                    state
                        .callback_frames
                        .fetch_add((data.len() / channels) as u64, Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        let value = if queue.paused {
                            0.0
                        } else if let Some(entry) = queue.queue.pop_front() {
                            state
                                .current_sentence
                                .store(entry.sentence as usize, Ordering::Relaxed);
                            state
                                .current_unit
                                .store(entry.unit as usize, Ordering::Relaxed);
                            from_sample(to_sample(entry.sample as f32 / 32768.0))
                        } else {
                            0.0
                        };
                        for sample in frame {
                            *sample = to_sample(value)
                        }
                    }
                    drop(queue);
                } else {
                    state.callback_contention.fetch_add(1, Ordering::Relaxed);
                    for sample in data.iter_mut() {
                        *sample = to_sample(0.0)
                    }
                }
                state.changed.notify_one();
            },
            move |error| {
                if error_state.stream_epoch.load(Ordering::Acquire) != epoch {
                    return;
                }
                if !error_state
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .stream_open
                    || error_state.stream_epoch.load(Ordering::Acquire) != epoch
                {
                    return;
                }
                let message = error.to_string();
                let recoverable = message.to_ascii_lowercase().contains("underrun")
                    || message.to_ascii_lowercase().contains("overrun");
                let first = error_state.callback_errors.fetch_add(1, Ordering::Relaxed) == 0;
                if recoverable {
                    if first {
                        tracing::warn!(%message,"recoverable CPAL audio underrun")
                    }
                } else {
                    *error_state
                        .fatal_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.clone());
                    if !error_state.fatal_reported.swap(true, Ordering::Relaxed) {
                        tracing::error!(%message,"fatal CPAL audio backend error")
                    }
                    let mut audio = error_state
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    audio.queue.clear();
                    audio.producer_done = true;
                    drop(audio);
                    error_state.changed.notify_all();
                }
            },
            None,
        )
        .context("opening audio output stream")
}
fn resample(input: &[i16], input_rate: u32, output_rate: u32) -> Vec<i16> {
    if input_rate == output_rate {
        return input.to_vec();
    }
    if input.is_empty() {
        return Vec::new();
    }
    let count = (input.len() as u64 * output_rate as u64 / input_rate as u64)
        .min(output_rate as u64 * 30) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let pos = i as f64 * input_rate as f64 / output_rate as f64;
        let left = (pos.floor() as usize).min(input.len() - 1);
        let right = (left + 1).min(input.len() - 1);
        let frac = (pos - left as f64) as f32;
        let a = input[left] as f32;
        let b = input[right] as f32;
        out.push((a * (1.0 - frac) + b * frac).round() as i16)
    }
    out
}

impl App {
    fn begin_repeat_action(&self, kind: &str) -> u64 {
        self.inner
            .repeat_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin(kind, unix_epoch_ms())
    }

    fn complete_repeat_action(&self, revision: u64, payload: repeat::RepeatPayload) {
        self.inner
            .repeat_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .complete(revision, payload);
    }

    #[cfg(target_os = "linux")]
    fn refresh_tray(&self) {
        if let Some(handle) = self
            .inner
            .tray_handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            let _ = handle.update(|_| {});
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn refresh_tray(&self) {}

    fn new(store: ConfigStore) -> Self {
        let events = model_events::ModelEvents::default();
        Self {
            inner: Arc::new(AppInner {
                store,
                model_events: events.clone(),
                route_language: Mutex::new(None),
                activity: Mutex::new(activity::Activity::default()),
                ocr_activity: Mutex::new(activity::Activity::default()),
                speculative_epoch: AtomicU64::new(0),
                copy_epoch: AtomicU64::new(0),
                region_gate: Mutex::new(()),
                generation: GenerationGate::default(),
                drafts: Mutex::new(HashMap::new()),
                draft_timer: lease_timer::LeaseTimer::default(),
                session: Mutex::new(None),
                audio: Arc::new(AudioOutput::new()),
                tts: Arc::new(WorkerSupervisor::new(events.clone())),
                media_bridge: Mutex::new(None),
                warmups: Mutex::new(HashMap::new()),
                server_jobs: Mutex::new(HashMap::new()),
                server_job_gate: Mutex::new(()),
                active_server_job: Mutex::new(None),
                server_job_generation: AtomicU64::new(0),
                server_unload_epoch: AtomicU64::new(0),
                server_idle_epoch: AtomicU64::new(0),
                server_idle_timer: lease_timer::LeaseTimer::default(),
                capture_kill: Mutex::new(None),
                intent_gate: Mutex::new(()),
                idle_deadline: Mutex::new(None),
                idle_lease_epoch: AtomicU64::new(0),
                idle_timer: lease_timer::LeaseTimer::default(),
                pause_timer: lease_timer::LeaseTimer::default(),
                ocr_timer: lease_timer::LeaseTimer::default(),
                capture_epoch: AtomicU64::new(0),
                ocr_prewarm_epoch: AtomicU64::new(0),
                #[cfg(target_os = "linux")]
                clipboard_owner: Mutex::new(None),
                clipboard_listening: AtomicBool::new(false),
                clipboard_listener_epoch: AtomicU64::new(0),
                clipboard_baseline: Mutex::new(None),
                clipboard_change_gate: Mutex::new(()),
                clipboard_change_running: AtomicBool::new(false),
                clipboard_change_pending: AtomicBool::new(false),
                clipboard_suppression: AtomicUsize::new(0),
                clipboard_read_generation: AtomicU64::new(0),
                #[cfg(target_os = "linux")]
                clipboard_watch_kill: Mutex::new(None),
                #[cfg(target_os = "linux")]
                tray_handle: Mutex::new(None),
                pause_lifecycle_gate: Mutex::new(()),
                pause_lease_epoch: AtomicU64::new(0),
                paused_restart_generation: AtomicU64::new(0),
                ocr_kill: Mutex::new(None),
                ocr_worker: Mutex::new(None),
                ocr_residency: RwLock::new(None),
                ocr_epoch: AtomicU64::new(0),
                ocr_gate: Mutex::new(()),
                shutdown: AtomicBool::new(false),
                server: Mutex::new(None),
                hud: Mutex::new(None),
                desktop_enabled: AtomicBool::new(true),
                notice: Mutex::new(ControllerNotice {
                    text: String::new(),
                    revision: 0,
                    expires: None,
                }),
                read_pending: AtomicBool::new(false),
                read_along_active: AtomicBool::new(false),
                read_along: Mutex::new(None),
                repeat_action: Mutex::new(repeat::RepeatActionState::default()),
            }),
        }
    }
    fn set_notice(&self, text: String, duration: Duration) {
        let mut notice = self
            .inner
            .notice
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        notice.revision = notice.revision.wrapping_add(1).max(1);
        notice.text = text;
        notice.expires = if duration.is_zero() {
            None
        } else {
            Some(Instant::now() + duration)
        };
    }
    fn suppress_clipboard_watcher(&self) -> ClipboardSuppressionGuard<'_> {
        self.inner
            .clipboard_suppression
            .fetch_add(1, Ordering::AcqRel);
        ClipboardSuppressionGuard(self)
    }
    fn clear_idle_lease(&self) {
        self.inner.idle_lease_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.idle_timer.cancel();
        *self
            .inner
            .idle_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
    fn idle_worker_incarnation(&self) -> Option<String> {
        if self.inner.read_pending.load(Ordering::Acquire)
            || self
                .inner
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
            || !self
                .inner
                .drafts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
            || !audio_is_idle(&self.inner.audio)
            || self
                .inner
                .capture_kill
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        {
            return None;
        }
        #[cfg(target_os = "linux")]
        if self
            .inner
            .clipboard_owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
        {
            return None;
        }
        self.inner.tts.idle_incarnation()
    }
    fn report_intent_failure(
        &self,
        generation: u64,
        label: &str,
        error: &anyhow::Error,
        keep_pending: bool,
    ) {
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.inner.generation.accepts(generation) {
            return;
        }
        if label.starts_with("Read-along") {
            self.inner.read_along_active.store(false, Ordering::Release);
        }
        self.clear_idle_lease();
        if !keep_pending {
            self.inner.read_pending.store(false, Ordering::Release);
        }
        self.inner.activity.lock().unwrap().set(generation, "error");
        tracing::warn!(%error, label, "reader request failed");
        let detail = error.to_string();
        let detail: String = detail.chars().take(320).collect();
        let hint = if label.starts_with("Read-along") {
            " Resume or Retry last action retries the saved area."
        } else {
            ""
        };
        self.set_notice(format!("{label}: {detail}{hint}"), Duration::from_secs(6));
        drop(intent);
        self.show_hud();
        if !keep_pending {
            self.schedule_idle_lease_if_current();
        }
    }
    fn show_hud(&self) {
        if !self.inner.desktop_enabled.load(Ordering::Relaxed) {
            return;
        }
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
            return;
        }
        let mut hud = self
            .inner
            .hud
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if hud
            .as_ref()
            .is_some_and(|running| running.load(Ordering::Acquire))
        {
            return;
        }
        let Ok(exe) = sibling("reader-hud") else {
            return;
        };
        let Ok(mut child) = Command::new(exe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let running = Arc::new(AtomicBool::new(true));
        *hud = Some(Arc::clone(&running));
        std::thread::spawn(move || {
            let _ = child.wait();
            running.store(false, Ordering::Release);
        });
    }
    fn begin_read_intent(&self) -> Result<u64> {
        self.begin_read_intent_with_pending(false)
    }
    fn begin_read_intent_with_pending(&self, pending: bool) -> Result<u64> {
        self.begin_read_intent_if_generation(pending, None)
    }
    fn begin_read_intent_if_generation(&self, pending: bool, expected: Option<u64>) -> Result<u64> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.pause_lease_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.pause_timer.cancel();
        self.inner
            .paused_restart_generation
            .store(0, Ordering::Release);
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if expected.is_some_and(|generation| !self.inner.generation.accepts(generation)) {
            bail!("clipboard change was superseded by a newer read");
        }
        let route_config = self.inner.store.load()?;
        let follows = route_config.speech.selection.mode
            == reader_core::routing::SelectionMode::FollowKeyboard
            || route_config.ocr.selection.mode
                == reader_core::routing::SelectionMode::FollowKeyboard;
        *self.inner.route_language.lock().unwrap() = if follows {
            query_keyboard_language().ok()
        } else {
            None
        };
        if let Some(mut run) = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            run.controller.cancel();
        }
        let generation = self.inner.generation.advance();
        self.inner.draft_timer.cancel();
        self.inner.activity.lock().unwrap().begin(generation);
        self.clear_idle_lease();
        self.inner.read_pending.store(pending, Ordering::Release);
        self.set_notice(String::new(), Duration::ZERO);
        self.inner.audio.stop(generation);
        self.inner.audio.reset_cursor();
        let previous_session = self
            .inner
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let cancel_result = self.inner.tts.cancel_job(
            previous_session
                .as_ref()
                .map(|session| session.job_id.as_str()),
            previous_session
                .as_ref()
                .map(|session| session.generation)
                .unwrap_or(generation.saturating_sub(1)),
            previous_session.is_none()
                || self
                    .inner
                    .store
                    .load()
                    .map(|config| {
                        matches!(
                            config.speech.provider,
                            SpeechProvider::OpenAiCompatible { .. }
                        )
                    })
                    .unwrap_or(false),
        );
        let needs_reap = match cancel_result {
            Ok(needs_reap) => needs_reap,
            Err(error) => {
                tracing::warn!(%error,"could not cancel active speech worker on new read intent");
                false
            }
        };
        drop(intent);
        if needs_reap {
            tracing::debug!(generation, "reaping terminated speech worker");
        }
        if let Err(error) = self.kill_capture() {
            tracing::warn!(%error,"could not stop prior screen capture on new read intent");
        }
        #[cfg(target_os = "linux")]
        if let Err(error) = self.kill_clipboard_owner() {
            tracing::warn!(%error,"could not stop prior clipboard transaction on new read intent");
        }
        if let Err(error) = self.kill_ocr() {
            tracing::warn!(%error,"could not stop prior OCR on new read intent");
        }
        *self
            .inner
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.inner
            .drafts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        drop(lifecycle);
        if needs_reap {
            let inner = Arc::clone(&self.inner);
            std::thread::spawn(move || {
                if inner.tts.reap_retiring() {
                    App { inner }.show_hud();
                }
            });
        } else if !pending {
            self.schedule_idle_lease_if_current();
        }
        Ok(generation)
    }
    fn start_text_at(&self, source: String, text: String, generation: u64) -> Result<String> {
        self.inner
            .activity
            .lock()
            .unwrap()
            .set(generation, "detecting_language");
        let saved = self.inner.store.load()?;
        let config =
            saved.routed_text(&text, self.inner.route_language.lock().unwrap().as_deref())?;
        if saved.speech.selection.mode == reader_core::routing::SelectionMode::AutoDetect {
            *self.inner.route_language.lock().unwrap() = Some(config.speech.language.clone());
        }
        {
            let mut activity = self.inner.activity.lock().unwrap();
            if activity.generation == generation {
                activity.language = Some(config.speech.language.clone());
                activity.model = Path::new(&config.speech.model_dir)
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.runtime = Path::new(&config.speech.runtime_dir)
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.set(generation, "synthesizing");
            }
        }
        let units = plan_units(&text, 240)?;
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        anyhow::ensure!(
            self.inner.generation.accepts(generation),
            "reading request was superseded"
        );
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        anyhow::ensure!(
            self.inner.generation.accepts(generation),
            "reading request was superseded"
        );
        let first_route =
            if config.speech.selection.mode == reader_core::routing::SelectionMode::AutoDetect {
                units
                    .first()
                    .and_then(|unit| reader_core::language::detect(&unit.text))
                    .and_then(|language| {
                        config
                            .routed(reader_core::routing::Feature::Tts, Some(&language))
                            .ok()
                    })
                    .unwrap_or_else(|| config.clone())
            } else {
                config.clone()
            };
        self.inner.speculative_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner
            .tts
            .resolve_speculation(generation, &first_route)?;
        self.inner.read_pending.store(false, Ordering::Release);
        drop(intent);
        self.inner.audio.replace(generation);
        let job_id = Uuid::new_v4().to_string();
        let sentence = units.first().map(|u| u.sentence_index).unwrap_or(0);
        let session = Session {
            job_id: job_id.clone(),
            generation,
            source: source.clone(),
            preparation: Arc::new(chunk_stream::Progress::new(units.len())),
            units,
        };
        *self.inner.session.lock().unwrap() = Some(session.clone());
        drop(lifecycle);
        self.show_hud();
        let app = self.clone();
        std::thread::spawn(move || app.run_session(config, session, sentence));
        Ok(format!("Reading started · {}", text_preview(&text)))
    }
    fn run_session(&self, config: AppConfig, session: Session, from_sentence: usize) {
        let start = Instant::now();
        let mut failure = None;
        let first_unit = session
            .units
            .iter()
            .position(|unit| unit.sentence_index >= from_sentence)
            .unwrap_or(session.units.len());
        for index in first_unit..session.units.len() {
            let unit = &session.units[index];
            if !self.inner.generation.accepts(session.generation) {
                break;
            }
            let unit_config = match media_routing::unit_route(&config, &unit.text) {
                Ok(config) => config,
                Err(error) => {
                    failure = Some(error.to_string());
                    break;
                }
            };
            if prepare_speech_text(&unit_config, &unit.text, unit.starts_in_code)
                .trim()
                .is_empty()
            {
                continue;
            }
            {
                let mut activity = self.inner.activity.lock().unwrap();
                if activity.generation == session.generation {
                    activity.language = Some(unit_config.speech.language.clone());
                    activity.model = Path::new(&unit_config.speech.model_dir)
                        .file_name()
                        .map(|v| v.to_string_lossy().into_owned());
                    activity.runtime = Path::new(&unit_config.speech.runtime_dir)
                        .file_name()
                        .map(|v| v.to_string_lossy().into_owned());
                }
            }
            self.inner
                .tts
                .desktop_priority_waiters
                .fetch_add(1, Ordering::AcqRel);
            let _desktop_priority = DesktopTtsPriority(&self.inner.tts.desktop_priority_waiters);
            if let Err(error) = self
                .inner
                .tts
                .synthesize(self, &unit_config, &session, unit)
            {
                if self.inner.generation.accepts(session.generation) {
                    failure = Some(error.to_string())
                };
                break;
            }
            if !self.inner.generation.accepts(session.generation) {
                break;
            }
            session.preparation.mark(index);
            if let Err(error) = reading_hint::record(&unit_config) {
                tracing::debug!(%error, "could not save last voice route");
            }
            if session
                .units
                .get(index + 1)
                .is_some_and(|next| next.sentence_index != unit.sentence_index)
            {
                let gap_samples = self.inner.audio.output_rate() as usize
                    * config.playback.sentence_gap_ms as usize
                    / 1000;
                if gap_samples > 0 {
                    let silence = vec![0; gap_samples];
                    if let Err(error) = self.inner.audio.push(
                        AudioFrame {
                            generation: session.generation,
                            sentence: unit.sentence_index as u32,
                            unit: unit.unit_index as u32,
                            samples: &silence,
                        },
                        &config.playback,
                        || self.inner.generation.accepts(session.generation),
                    ) {
                        failure = Some(error.to_string());
                        break;
                    }
                }
            }
        }
        self.inner.audio.finish(session.generation);
        if let Err(error) = self.inner.audio.wait_drained(session.generation) {
            failure = Some(error.to_string())
        }
        if self.inner.generation.accepts(session.generation) {
            let mut current = self.inner.session.lock().unwrap();
            if current
                .as_ref()
                .is_some_and(|active| active.generation == session.generation)
            {
                *current = None;
            }
        }
        if self.inner.generation.accepts(session.generation) {
            self.inner.activity.lock().unwrap().set(
                session.generation,
                if failure.is_some() { "error" } else { "idle" },
            );
        }
        if let Some(error) = failure.filter(|_| self.inner.generation.accepts(session.generation)) {
            let message = if config.privacy.hide_text_in_notifications {
                "Reading stopped".to_owned()
            } else {
                format!("Reading stopped: {error}")
            };
            self.set_notice(message, Duration::from_secs(4));
            self.show_hud();
            tracing::error!(%error,"reading session failed");
        }
        self.clear_clipboard_read_generation(session.generation);
        if self.inner.generation.accepts(session.generation) {
            self.schedule_unload(config.playback.keep_warm_seconds, session.generation);
        } else {
            self.schedule_idle_lease_if_current();
        }
        tracing::info!(elapsed_ms=start.elapsed().as_millis(),source=%session.source,generation=session.generation,"reading producer finished");
    }
    fn schedule_unload(&self, seconds: u16, generation: u64) {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.inner.generation.accepts(generation) {
            return;
        }
        let Some(incarnation) = self.idle_worker_incarnation() else {
            return;
        };
        let lease = self.inner.idle_lease_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
        *self
            .inner
            .idle_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(deadline);
        let worker = Arc::clone(&self.inner);
        self.inner.idle_timer.schedule(deadline, move || {
            let lifecycle = worker
                .pause_lifecycle_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let intent = worker
                .intent_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if worker.idle_lease_epoch.load(Ordering::Acquire) != lease {
                return;
            }
            #[cfg(target_os = "linux")]
            let no_clipboard_owner = worker
                .clipboard_owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none();
            #[cfg(not(target_os = "linux"))]
            let no_clipboard_owner = true;
            let retired = worker.tts.retire_if(Some(&incarnation), || {
                worker.generation.accepts(generation)
                    && worker.idle_lease_epoch.load(Ordering::Acquire) == lease
                    && !worker.read_pending.load(Ordering::Acquire)
                    && audio_is_idle(&worker.audio)
                    && worker
                        .session
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .is_none()
                    && !server_tts_in_use(&worker)
                    && worker.drafts.lock().unwrap().is_empty()
                    && worker.capture_kill.lock().unwrap().is_none()
                    && no_clipboard_owner
            });
            drop(intent);
            drop(lifecycle);
            if matches!(retired, Ok(true)) {
                worker.idle_lease_epoch.fetch_add(1, Ordering::AcqRel);
                *worker.idle_deadline.lock().unwrap() = None;
                write_device_report("Stopped · lease expired", false);
                App {
                    inner: Arc::clone(&worker),
                }
                .show_hud();
            } else if let Err(error) = retired {
                tracing::warn!(%error,"idle voice worker cleanup failed");
            } else {
                let mut deadline = worker.idle_deadline.lock().unwrap();
                if worker.idle_lease_epoch.load(Ordering::Acquire) == lease {
                    *deadline = None;
                }
            }
        });
        drop(intent);
        drop(lifecycle);
    }
    fn schedule_idle_lease_if_current(&self) {
        self.schedule_configured_unload(self.inner.generation.current());
    }
    fn schedule_configured_unload(&self, generation: u64) {
        if let Ok(config) = self.inner.store.load() {
            self.schedule_unload(config.playback.keep_warm_seconds, generation);
        }
    }
    fn schedule_paused_release(&self, generation: u64, lease: u64) {
        let inner = Arc::clone(&self.inner);
        let incarnation = inner.tts.incarnation();
        let timeout = inner
            .store
            .load()
            .map(|config| config.playback.keep_warm_seconds)
            .unwrap_or(300);
        self.inner.pause_timer.schedule(
            Instant::now() + Duration::from_secs(u64::from(timeout)),
            move || {
                let lifecycle = inner
                    .pause_lifecycle_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if inner.pause_lease_epoch.load(Ordering::Acquire) != lease
                    || !inner.generation.accepts(generation)
                {
                    return;
                }
                let mut session = inner
                    .session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(mut preserved) = session.clone() else {
                    return;
                };
                if preserved.generation != generation {
                    return;
                }
                let intent = inner
                    .intent_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(next_generation) = inner.generation.advance_from(generation) else {
                    return;
                };
                drop(intent);
                preserved.generation = next_generation;
                inner.audio.stop(next_generation);
                if !inner.generation.accepts(next_generation) {
                    return;
                }
                inner.audio.pause(true, next_generation);
                *session = Some(preserved);
                inner
                    .paused_restart_generation
                    .store(next_generation, Ordering::Release);
                drop(session);
                drop(lifecycle);
                if let Some(incarnation) = incarnation {
                    release_paused_worker(inner, incarnation, next_generation, lease);
                }
            },
        );
    }
    fn action(&self, action: &str) -> Result<String> {
        match action {
            "read-selection" => self.action_read_selection(),
            "copy-region" => self.action_copy_region(),
            "read-region" => self.action_read_region(),
            "read-along" => self.action_read_along(),
            "read-along-next" => self.action_read_along_next(),
            "read-along-stop" => self.action_stop_read_along(),
            "repeat" => self.action_repeat(),
            "toggle-clipboard-listening" => self.action_toggle_clipboard_listening(),
            "clipboard-changed" => {
                self.request_clipboard_scan();
                Ok("Clipboard change noticed".into())
            }
            "prepare" => self.action_prepare(),
            "pause" => self.action_pause(),
            "resume" => self.action_resume(),
            "stop" => self.action_stop(),
            "unload" => self.action_unload(),
            "refresh_idle_lease" => self.action_refresh_idle_lease(),
            "server_start" => self.action_server_start(),
            "server_stop" => self.action_server_stop(),
            "replay" | "next" | "previous" => self.action_navigate(action),
            _ => bail!("unsupported controller action '{action}'"),
        }
    }
    fn action_repeat(&self) -> Result<String> {
        let (_kind, payload) = self
            .inner
            .repeat_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .repeat(unix_epoch_ms())?;
        match payload {
            repeat::RepeatPayload::Unavailable { reason } => {
                bail!("the latest reader action cannot be repeated: {reason}")
            }
            repeat::RepeatPayload::Text { source, text } => {
                let generation = self.begin_read_intent_with_pending(true)?;
                self.show_hud();
                self.start_text_at(source, text, generation)
            }
            repeat::RepeatPayload::Region { action, target } => {
                if action == repeat::RegionAction::Copy {
                    let language = self.inner.store.load().ok().and_then(|config| {
                        (config.ocr.selection.mode
                            == reader_core::routing::SelectionMode::FollowKeyboard)
                            .then(|| query_keyboard_language().ok())
                            .flatten()
                    });
                    let epoch = self.inner.copy_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    let app = self.clone();
                    std::thread::spawn(move || {
                        if let Err(error) = copy_region_with_target(&app, epoch, language, target) {
                            if app.inner.copy_epoch.load(Ordering::Acquire) == epoch {
                                tracing::warn!(%error, "repeated OCR copy failed");
                                app.set_notice(
                                    format!("OCR copy repeat failed: {error}"),
                                    Duration::from_secs(4),
                                );
                                app.show_hud();
                            }
                        }
                    });
                    Ok("Repeated OCR copy started".into())
                } else {
                    let generation = self.begin_read_intent_with_pending(true)?;
                    self.show_hud();
                    let app = self.clone();
                    std::thread::spawn(move || {
                        let current = || {
                            app.inner.generation.accepts(generation)
                                && !app.inner.shutdown.load(Ordering::Acquire)
                        };
                        let result = (|| -> Result<()> {
                            let image = target.capture(&current)?;
                            let _image_cleanup = privacy::TemporaryFile(image.clone());
                            anyhow::ensure!(current(), "screen capture was canceled");
                            app.read_image_at(
                                image.to_str().context("invalid captured image path")?,
                                generation,
                            )?;
                            Ok(())
                        })();
                        if let Err(error) = result {
                            app.report_intent_failure(
                                generation,
                                "Screen read repeat failed",
                                &error,
                                false,
                            );
                        }
                    });
                    Ok("Repeated screen-region read started".into())
                }
            }
            repeat::RepeatPayload::ReadAlong { target } => self.start_read_along(Some(target)),
        }
    }
    #[cfg(any(target_os = "linux", windows))]
    fn action_copy_region(&self) -> Result<String> {
        let repeat_revision = self.begin_repeat_action("copy-region");
        let config = self.inner.store.load()?;
        let language =
            if config.ocr.selection.mode == reader_core::routing::SelectionMode::FollowKeyboard {
                query_keyboard_language().ok()
            } else {
                None
            };
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let epoch = self.inner.copy_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let app = self.clone();
        std::thread::spawn(move || {
            if let Err(error) =
                copy_region_inner(&app, epoch, language, None, Some(repeat_revision))
            {
                if app.inner.copy_epoch.load(Ordering::Acquire) == epoch {
                    tracing::warn!(%error, "OCR copy failed");
                    app.set_notice(format!("OCR copy failed: {error}"), Duration::from_secs(4));
                    app.show_hud();
                }
            }
        });
        drop(intent);
        Ok("OCR capture started".into())
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn action_copy_region(&self) -> Result<String> {
        bail!("screen-region capture is not available on this platform")
    }
    fn action_toggle_clipboard_listening(&self) -> Result<String> {
        let enabled = !self.inner.clipboard_listening.load(Ordering::Acquire);
        self.set_clipboard_listening(enabled)?;
        Ok(if enabled {
            "Clipboard listening enabled".into()
        } else {
            "Clipboard listening disabled".into()
        })
    }
    fn set_clipboard_listening(&self, enabled: bool) -> Result<()> {
        let gate = self
            .inner
            .clipboard_change_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let was_enabled = self.inner.clipboard_listening.load(Ordering::Acquire);
        if was_enabled == enabled {
            return Ok(());
        }
        let epoch = self
            .inner
            .clipboard_listener_epoch
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        if enabled {
            *self
                .inner
                .clipboard_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = read_clipboard_snapshot()
                .ok()
                .map(|snapshot| snapshot.fingerprint);
            self.inner
                .clipboard_listening
                .store(true, Ordering::Release);
            drop(gate);
            if let Err(error) = start_clipboard_listener(self.clone(), epoch) {
                self.inner
                    .clipboard_listening
                    .store(false, Ordering::Release);
                self.inner
                    .clipboard_listener_epoch
                    .fetch_add(1, Ordering::AcqRel);
                self.stop_clipboard_listener();
                return Err(error);
            }
            self.set_notice("Clipboard listening enabled".into(), Duration::from_secs(2));
            self.refresh_tray();
            self.show_hud();
            return Ok(());
        }
        self.inner
            .clipboard_listening
            .store(false, Ordering::Release);
        *self
            .inner
            .clipboard_baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let watched_generation = self
            .inner
            .clipboard_read_generation
            .swap(0, Ordering::AcqRel);
        drop(gate);
        self.stop_clipboard_listener();
        if watched_generation != 0 {
            let _ = self.begin_read_intent_if_generation(false, Some(watched_generation));
        }
        self.set_notice(
            "Clipboard listening disabled".into(),
            Duration::from_secs(2),
        );
        self.refresh_tray();
        self.show_hud();
        Ok(())
    }
    fn stop_clipboard_listener(&self) {
        #[cfg(target_os = "linux")]
        if let Some(identity) = self
            .inner
            .clipboard_watch_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            if let Err(error) = identity.terminate("clipboard watcher") {
                tracing::debug!(%error, "clipboard watcher already exited");
            }
        }
    }
    fn request_clipboard_scan(&self) {
        if !self.inner.clipboard_listening.load(Ordering::Acquire) {
            return;
        }
        self.inner
            .clipboard_change_pending
            .store(true, Ordering::Release);
        if self
            .inner
            .clipboard_change_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let app = self.clone();
        std::thread::spawn(move || loop {
            app.inner
                .clipboard_change_pending
                .store(false, Ordering::Release);
            if app.inner.clipboard_listening.load(Ordering::Acquire) {
                if let Err(error) = app.handle_clipboard_change() {
                    tracing::warn!(%error, "clipboard change could not be read");
                }
            }
            if app
                .inner
                .clipboard_change_pending
                .swap(false, Ordering::AcqRel)
            {
                continue;
            }
            app.inner
                .clipboard_change_running
                .store(false, Ordering::Release);
            if app.inner.clipboard_change_pending.load(Ordering::Acquire)
                && app
                    .inner
                    .clipboard_change_running
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                continue;
            }
            break;
        });
    }
    fn rebaseline_clipboard(&self) {
        if !self.inner.clipboard_listening.load(Ordering::Acquire) {
            return;
        }
        let _gate = self
            .inner
            .clipboard_change_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(snapshot) = read_clipboard_snapshot() {
            *self
                .inner
                .clipboard_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot.fingerprint);
        }
    }
    fn clear_clipboard_read_generation(&self, generation: u64) {
        let _ = self.inner.clipboard_read_generation.compare_exchange(
            generation,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    fn handle_clipboard_change(&self) -> Result<()> {
        if !self.inner.clipboard_listening.load(Ordering::Acquire) {
            return Ok(());
        }
        let gate = self
            .inner
            .clipboard_change_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.inner.clipboard_listening.load(Ordering::Acquire) {
            return Ok(());
        }
        let observed_generation = self.inner.generation.current();
        let snapshot = match read_clipboard_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::debug!(%error, "clipboard change has no supported content");
                return Ok(());
            }
        };
        let previous = self
            .inner
            .clipboard_baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(snapshot.fingerprint);
        if previous == Some(snapshot.fingerprint)
            || self.inner.clipboard_suppression.load(Ordering::Acquire) > 0
        {
            return Ok(());
        }
        let Some(payload) = snapshot.payload else {
            return Ok(());
        };
        let generation = match self.begin_read_intent_if_generation(true, Some(observed_generation))
        {
            Ok(generation) => generation,
            Err(_) => return Ok(()),
        };
        self.inner
            .clipboard_read_generation
            .store(generation, Ordering::Release);
        drop(gate);
        let result = match payload {
            ClipboardPayload::Text(text) => {
                self.start_text_at("clipboard".into(), text, generation)
            }
            ClipboardPayload::Image { bytes, extension } => {
                let image = match store_clipboard_image(&bytes, extension) {
                    Ok(image) => image,
                    Err(error) => {
                        self.report_intent_failure(
                            generation,
                            "Clipboard image failed",
                            &error,
                            false,
                        );
                        self.clear_clipboard_read_generation(generation);
                        return Err(error);
                    }
                };
                let app = self.clone();
                std::thread::spawn(move || {
                    let result = app.read_image_at_source(&image, generation, "clipboard image");
                    let _ = std::fs::remove_file(image);
                    if let Err(error) = result {
                        app.report_intent_failure(
                            generation,
                            "Clipboard image failed",
                            &error,
                            false,
                        );
                        app.clear_clipboard_read_generation(generation);
                    }
                });
                Ok("Clipboard image OCR started".into())
            }
        };
        if let Err(error) = &result {
            self.report_intent_failure(generation, "Clipboard read failed", error, false);
            self.clear_clipboard_read_generation(generation);
        }
        result.map(|_| ())
    }
    fn action_read_selection(&self) -> Result<String> {
        let generation = self.begin_read_intent_with_pending(true)?;
        self.show_hud();
        let app = self.clone();
        std::thread::spawn(move || {
            if app.inner.generation.accepts(generation) {
                if let Err(error) = read_selection_hotkey_at(&app, generation) {
                    tracing::warn!(%error, "selection copy failed");
                }
            }
        });
        Ok("Selection copy started".into())
    }
    fn action_read_region(&self) -> Result<String> {
        let generation = self.begin_read_intent_with_pending(true)?;
        self.show_hud();
        let app = self.clone();
        std::thread::spawn(move || {
            if app.inner.generation.accepts(generation) {
                if let Err(error) = read_region_hotkey_at(&app, generation) {
                    tracing::warn!(%error, "screen-region capture failed");
                }
            }
        });
        Ok("Screen-region selector opened".into())
    }
    fn action_read_along(&self) -> Result<String> {
        if self.inner.read_along_active.load(Ordering::Acquire) {
            return self.action_stop_read_along();
        }
        self.start_read_along(None)
    }
    fn start_read_along(&self, saved_target: Option<read_along_capture::Target>) -> Result<String> {
        let config = self.inner.store.load()?;
        let repeat_revision = self.begin_repeat_action("read-along");
        let generation = self.begin_read_intent_with_pending(true)?;
        self.inner.read_along_active.store(true, Ordering::Release);
        if let Some(target) = &saved_target {
            self.complete_repeat_action(
                repeat_revision,
                repeat::RepeatPayload::ReadAlong {
                    target: target.clone(),
                },
            );
        }
        self.inner
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set(generation, "selecting_region");
        self.show_hud();
        let app = self.clone();
        std::thread::spawn(move || {
            let current = || app.inner.generation.accepts(generation);
            let selected = match saved_target {
                Some(target) => target.capture(&current).map(|image| (target, image)),
                None => read_along_capture::Target::select(current),
            };
            let (target, image) = match selected {
                Ok(selected) => selected,
                Err(error) => {
                    app.report_intent_failure(
                        generation,
                        "Read-along selection failed",
                        &error,
                        false,
                    );
                    return;
                }
            };
            let _image_cleanup = privacy::TemporaryFile(image.clone());
            if !app.inner.generation.accepts(generation) {
                return;
            }
            app.complete_repeat_action(
                repeat_revision,
                repeat::RepeatPayload::ReadAlong {
                    target: target.clone(),
                },
            );
            let action = match config.read_along.action {
                reader_core::config::ReadAlongAction::Key => config
                    .read_along
                    .sequence
                    .clone()
                    .map(read_along_capture::AdvanceAction::Sequence)
                    .unwrap_or_else(|| {
                        read_along_capture::AdvanceAction::Key(
                            config.read_along.advance_key.clone(),
                        )
                    }),
                reader_core::config::ReadAlongAction::Click => {
                    read_along_capture::AdvanceAction::Click {
                        x: target.region.width / 2,
                        y: target.region.height / 2,
                    }
                }
            };
            if let Err(error) = target.prepare_input(&action, &current) {
                let _ = std::fs::remove_file(&image);
                app.report_intent_failure(
                    generation,
                    "Read-along input permission failed",
                    &error,
                    false,
                );
                return;
            }
            // Advance from the captured snapshot while its OCR and speech begin.
            let first_advance = config.read_along.prefetch.then(|| {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let app = app.clone();
                let target = target.clone();
                let settings = config.read_along.clone();
                std::thread::spawn(move || {
                    let current = || {
                        app.inner.generation.accepts(generation)
                            && app.inner.activity.lock().unwrap().phase != "paused"
                    };
                    let result = target
                        .advance(read_along_input(&settings, &target), &current, &|_| {})
                        .map(|_| Instant::now())
                        .map_err(|error| error.to_string());
                    let _ = tx.send(result);
                });
                rx
            });
            let text = match app.recognize_read_along_page(&target, image, generation) {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => {
                    let error = anyhow::anyhow!("the selected region contains no readable text");
                    app.report_intent_failure(generation, "Read-along OCR failed", &error, false);
                    return;
                }
                Err(error) => {
                    app.report_intent_failure(generation, "Read-along OCR failed", &error, false);
                    return;
                }
            };
            if let Err(error) = app.start_text_at("read-along".into(), text.clone(), generation) {
                tracing::warn!(%error, "starting read-along speech failed");
                app.report_intent_failure(generation, "Read-along start failed", &error, false);
                return;
            }
            let mut controller = read_along::Controller::new(config.read_along.retry_count);
            let read_along_generation = controller.generation();
            controller.set_current_page(read_along_generation, text_fingerprint(&text));
            controller.request_advance(read_along_generation, false, false);
            *app.inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ReadAlongRun {
                controller,
                target,
                app_generation: generation,
                settings: config.read_along.clone(),
                prepared: None,
                next_activity: None,
                next_progress: None,
                first_advance,
                preparation: read_along::Preparation::new(config.read_along.prefetch),
                active_stream: None,
            });
            if config.read_along.prefetch {
                app.start_read_along_prefetch(read_along_generation);
            }
            app.wait_for_read_along_audio(read_along_generation);
        });
        Ok("Read-along started".into())
    }
    fn failed_read_along_area_is_saved(&self) -> bool {
        let inactive = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none();
        let failed = self
            .inner
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .phase
            == "error";
        let saved = self
            .inner
            .repeat_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .has_read_along_target();
        inactive && failed && saved
    }
    fn action_read_along_next(&self) -> Result<String> {
        if self.failed_read_along_area_is_saved() {
            return self.action_repeat();
        }

        if audio_is_paused(&self.inner.audio) {
            self.action_resume()?;
            return Ok("Read-along resumed".into());
        }
        let failed = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|run| run.controller.failed());
        if failed {
            self.action_resume()?;
            return Ok("Retrying the pending read-along page".into());
        }
        let decision = {
            let mut run = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let run = run.as_mut().context("read-along is not active")?;
            let generation = run.controller.generation();
            let audio_finished = audio_is_idle(&self.inner.audio);
            run.controller
                .request_advance(generation, run.settings.prefetch, audio_finished)
        };
        match decision {
            read_along::AdvanceDecision::CaptureNextPage => {
                let generation = self
                    .inner
                    .read_along
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .controller
                    .generation();
                self.start_read_along_prefetch(generation);
                Ok("Preparing the next read-along page".into())
            }
            read_along::AdvanceDecision::PromotePreparedPage => {
                let generation = self
                    .inner
                    .read_along
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .controller
                    .generation();
                self.promote_read_along_page(generation)?;
                Ok("Read-along advanced".into())
            }
            read_along::AdvanceDecision::WaitForAudio => {
                let generation = self
                    .inner
                    .read_along
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .controller
                    .generation();
                self.wait_for_read_along_audio(generation);
                Ok("Read-along will advance when the current page finishes".into())
            }
            read_along::AdvanceDecision::WaitForPreparedPage => {
                Ok("Preparing the next read-along page".into())
            }
            read_along::AdvanceDecision::PauseAfterAudio => {
                self.action_pause()?;
                Ok("Read-along paused after the current page".into())
            }
            read_along::AdvanceDecision::Ignore => bail!("read-along advance was superseded"),
        }
    }
    fn action_stop_read_along(&self) -> Result<String> {
        if let Some(mut run) = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            run.controller.cancel();
        }
        self.action_stop()?;
        Ok("Read-along stopped".into())
    }
    fn start_read_along_prefetch(&self, generation: u64) {
        let (target, settings, app_generation, first_advance, advance, cached_text) = {
            let mut state = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(run) = state.as_mut() else { return };
            if run.controller.request_prefetch(generation)
                != read_along::AdvanceDecision::CaptureNextPage
            {
                return;
            }
            run.next_activity = Some(ReadAlongNextActivity {
                phase: "advancing",
                waiting_until: None,
            });
            (
                run.target.clone(),
                run.settings.clone(),
                run.app_generation,
                run.first_advance.take(),
                run.preparation.begin_advance(),
                run.preparation.text.clone(),
            )
        };
        let app = self.clone();
        std::thread::spawn(move || {
            let current = || app.read_along_is_current(generation, app_generation);
            let result = (|| -> Result<PreparedReadAlongPage> {
                while current() && app.inner.activity.lock().unwrap().phase == "paused" {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let advancing =
                    || current() && app.inner.activity.lock().unwrap().phase != "paused";
                let waiting = std::cell::Cell::new(false);
                let show_waiting = |value| {
                    if waiting.replace(value) != value && current() {
                        if value {
                            app.set_notice(
                                "Read-along waiting for selected window".into(),
                                Duration::from_secs(3600),
                            );
                        } else if app.inner.notice.lock().unwrap().text
                            == "Read-along waiting for selected window"
                        {
                            app.set_notice(String::new(), Duration::ZERO);
                        }
                        app.show_hud();
                    }
                };
                // Input is outside processing retries: delivery can fail after a partial action.
                let advanced = if let Some(pending) = first_advance {
                    receive_read_along(pending, &current)?
                } else {
                    if advance {
                        target.advance(
                            read_along_input(&settings, &target),
                            &advancing,
                            &show_waiting,
                        )?;
                    }
                    Instant::now()
                };
                let retrying = |attempt| {
                    if current() {
                        app.set_notice(
                            format!("Read-along retry {attempt}/{}", settings.retry_count),
                            Duration::from_secs(5),
                        );
                        app.show_hud();
                    }
                };
                let text = if let Some(text) = cached_text {
                    let mut state = app
                        .inner
                        .read_along
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let run = state
                        .as_mut()
                        .filter(|run| run.controller.accepts(generation))
                        .context("read-along was canceled")?;
                    anyhow::ensure!(
                        run.controller
                            .ocr_result(generation, text_fingerprint(&text))
                            == read_along::OcrDecision::Prepared,
                        "pending page was superseded"
                    );
                    text
                } else {
                    read_along::retry(
                        settings.retry_count,
                        &current,
                        || {
                            app.set_read_along_next_activity(generation, app_generation, "waiting");
                            if let Some(run) = app
                                .inner
                                .read_along
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .as_mut()
                                .filter(|run| run.controller.accepts(generation))
                            {
                                if let Some(activity) = &mut run.next_activity {
                                    activity.waiting_until = Some(
                                        advanced
                                            + Duration::from_millis(u64::from(settings.delay_ms)),
                                    );
                                }
                            }
                            wait_read_along_settle(advanced, settings.delay_ms, &current)?;
                            app.set_read_along_next_activity(
                                generation,
                                app_generation,
                                "capturing",
                            );
                            let image = target.capture(&current)?;
                            app.set_read_along_next_activity(generation, app_generation, "ocr");
                            let text =
                                app.recognize_read_along_page(&target, image, app_generation)?;
                            anyhow::ensure!(
                                !text.trim().is_empty(),
                                "no readable text in the selected region"
                            );
                            let route = app.inner.store.load()?.routed_text(
                                &text,
                                app.inner.route_language.lock().unwrap().as_deref(),
                            )?;
                            anyhow::ensure!(
                                plan_units(&text, 240)?
                                    .iter()
                                    .any(|unit| !prepare_speech_text(
                                        &route,
                                        &unit.text,
                                        unit.starts_in_code
                                    )
                                    .trim()
                                    .is_empty()),
                                "no readable speech remains after text cleanup"
                            );
                            let mut state = app
                                .inner
                                .read_along
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let run = state
                                .as_mut()
                                .filter(|run| run.controller.accepts(generation))
                                .context("read-along was canceled")?;
                            match run
                                .controller
                                .ocr_result(generation, text_fingerprint(&text))
                            {
                                read_along::OcrDecision::Prepared => {
                                    run.preparation.text = Some(text.clone());
                                    Ok(text)
                                }
                                read_along::OcrDecision::RetryUnchanged
                                | read_along::OcrDecision::PauseAfterAudio => {
                                    bail!("the selected region did not advance to new text")
                                }
                                read_along::OcrDecision::Ignore => {
                                    bail!("pending page was superseded")
                                }
                            }
                        },
                        retrying,
                    )?
                };
                app.set_read_along_next_activity(generation, app_generation, "tts");
                let units = plan_units(&text, 240)?;
                let progress = Arc::new(chunk_stream::Progress::new(units.len()));
                if let Some(run) = app
                    .inner
                    .read_along
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_mut()
                    .filter(|run| run.controller.accepts(generation))
                {
                    run.next_progress = Some(progress.clone());
                }
                let page_route = app
                    .inner
                    .store
                    .load()?
                    .routed_text(&text, app.inner.route_language.lock().unwrap().as_deref())?;
                let playing_generation = Arc::new(AtomicU64::new(0));
                let mut pending = units.into_iter().enumerate();
                let first = loop {
                    let (index, unit) = pending
                        .next()
                        .context("no readable speech remains after text cleanup")?;
                    let chunk = app.prepare_read_along_chunk(
                        &page_route,
                        &unit,
                        settings.retry_count,
                        false,
                        &current,
                    )?;
                    if !chunk.samples.is_empty() {
                        progress.mark(index);
                        break chunk;
                    }
                };
                let first_samples = first.samples.len();
                let (sender, remaining) = chunk_stream::channel();
                let producer = app.clone();
                let playing = playing_generation.clone();
                let producer_progress = progress.clone();
                std::thread::spawn(move || {
                    let current = || {
                        sender.active()
                            && producer.inner.read_along_active.load(Ordering::Acquire)
                            && producer
                                .inner
                                .read_along
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .as_ref()
                                .is_some_and(|run| run.controller.accepts(generation))
                    };
                    let mut total = first_samples;
                    for (index, unit) in pending {
                        if !current() {
                            return;
                        }
                        let priority = producer
                            .inner
                            .generation
                            .accepts(playing.load(Ordering::Acquire));
                        let chunk = producer
                            .prepare_read_along_chunk(
                                &page_route,
                                &unit,
                                settings.retry_count,
                                priority,
                                &current,
                            )
                            .and_then(|chunk| {
                                anyhow::ensure!(
                                    total.saturating_add(chunk.samples.len())
                                        <= media_routing::MAX_SAMPLES,
                                    "read-along page audio exceeds the five-minute limit"
                                );
                                Ok(chunk)
                            });
                        match chunk {
                            Ok(chunk) => {
                                if !chunk.samples.is_empty() {
                                    producer_progress.mark(index);
                                }
                                total += chunk.samples.len();
                                if !chunk.samples.is_empty()
                                    && !sender.send(Ok(Some(chunk)), &current)
                                {
                                    return;
                                }
                            }
                            Err(error) if current() => {
                                let _ = sender.send(Err(error.to_string()), &current);
                                return;
                            }
                            Err(_) => return,
                        }
                    }
                    sender.send(Ok(None), &current);
                });
                Ok(PreparedReadAlongPage {
                    text,
                    first,
                    progress,
                    remaining,
                    playing_generation,
                })
            })();
            let prepared = match result {
                Ok(prepared) if current() => prepared,
                Ok(_) => return,
                Err(error) if current() => {
                    app.read_along_failed(generation, app_generation, error);
                    return;
                }
                Err(_) => return,
            };
            let mut state = app
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(run) = state.as_mut().filter(|run| {
                run.app_generation == app_generation && run.controller.accepts(generation)
            }) else {
                return;
            };
            run.prepared = Some(prepared);
            run.next_activity = Some(ReadAlongNextActivity {
                phase: "ready",
                waiting_until: None,
            });
            if audio_is_idle(&app.inner.audio) {
                let decision = run.controller.audio_finished(generation);
                drop(state);
                if decision == read_along::AdvanceDecision::PromotePreparedPage {
                    if let Err(error) = app.promote_read_along_page(generation) {
                        app.read_along_failed(generation, app_generation, error);
                    }
                }
            }
        });
    }
    fn prepare_read_along_chunk(
        &self,
        page_route: &AppConfig,
        unit: &SpeechUnit,
        retries: u8,
        priority: bool,
        current: &dyn Fn() -> bool,
    ) -> Result<PreparedReadAlongChunk> {
        read_along::retry(
            retries,
            current,
            || {
                let route = media_routing::unit_route(page_route, &unit.text)?;
                if prepare_speech_text(&route, &unit.text, unit.starts_in_code)
                    .trim()
                    .is_empty()
                {
                    return Ok(PreparedReadAlongChunk {
                        unit: unit.clone(),
                        samples: Vec::new(),
                        route,
                    });
                }
                let (samples, rate) = self.inner.tts.collect(
                    &route,
                    &unit.text,
                    SynthesisOptions {
                        priority,
                        starts_in_code: unit.starts_in_code,
                        voice: &route.speech.voice,
                        speed: route.speech.speed,
                        request_id: None,
                        lifetime: None,
                        current,
                    },
                )?;
                anyhow::ensure!(!samples.is_empty(), "speech provider returned no audio");
                let mut pcm = Vec::new();
                media_routing::append_pcm(
                    &mut pcm,
                    &samples,
                    rate,
                    media_routing::OUTPUT_RATE as usize * 60,
                )?;
                Ok(PreparedReadAlongChunk {
                    unit: unit.clone(),
                    samples: pcm,
                    route,
                })
            },
            |attempt| {
                if current() {
                    self.set_notice(
                        format!("Read-along retry {attempt}/{retries}"),
                        Duration::from_secs(3),
                    );
                    self.show_hud();
                }
            },
        )
    }
    fn set_read_along_next_activity(
        &self,
        generation: u64,
        app_generation: u64,
        phase: &'static str,
    ) {
        let mut state = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(run) = state.as_mut().filter(|run| {
            run.app_generation == app_generation && run.controller.accepts(generation)
        }) {
            run.next_activity = Some(ReadAlongNextActivity {
                phase,
                waiting_until: None,
            });
        }
    }
    fn read_along_is_current(&self, generation: u64, app_generation: u64) -> bool {
        self.inner.generation.accepts(app_generation)
            && self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .is_some_and(|run| {
                    run.app_generation == app_generation && run.controller.accepts(generation)
                })
    }
    fn read_along_failed(&self, generation: u64, app_generation: u64, error: anyhow::Error) {
        let decision = {
            let mut state = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(run) = state
                .as_mut()
                .filter(|run| run.app_generation == app_generation)
            else {
                return;
            };
            if !run.controller.accepts(generation) {
                return;
            }
            run.prepared = None;
            run.next_activity = None;
            run.next_progress = None;
            run.controller.capture_failed(generation)
        };
        tracing::warn!(%error, "read-along next page failed");
        if decision == read_along::AdvanceDecision::PauseAfterAudio {
            self.finish_failed_read_along(generation, app_generation, &error.to_string());
        } else {
            // Keep current speech intact; the failed controller cannot promote a page.
            self.set_notice(
                format!("Read-along stopped: {error}. Use Retry last action for the saved area."),
                Duration::from_secs(5),
            );
            self.wait_for_read_along_audio(generation);
        }
    }
    fn finish_failed_read_along(&self, generation: u64, app_generation: u64, error: &str) {
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.inner.generation.accepts(app_generation)
            || !state.as_ref().is_some_and(|run| {
                run.app_generation == app_generation && run.controller.accepts(generation)
            })
        {
            return;
        }
        if let Some(mut run) = state.take() {
            run.controller.cancel();
        }
        self.inner.read_along_active.store(false, Ordering::Release);
        drop(state);
        *self.inner.session.lock().unwrap() = None;
        self.inner
            .activity
            .lock()
            .unwrap()
            .set(app_generation, "idle");
        self.set_notice(
            format!("Read-along stopped: {error}. Use Retry last action for the saved area."),
            Duration::from_secs(5),
        );
        drop(intent);
        self.show_hud();
    }
    fn wait_for_read_along_audio(&self, generation: u64) {
        let app = self.clone();
        std::thread::spawn(move || {
            while app
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .is_some_and(|run| {
                    run.controller.accepts(generation) && !audio_is_idle(&app.inner.audio)
                })
            {
                std::thread::sleep(Duration::from_millis(30));
            }
            let decision = {
                let mut state = app
                    .inner
                    .read_along
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(run) = state
                    .as_mut()
                    .filter(|run| run.controller.accepts(generation))
                else {
                    return;
                };
                run.controller.audio_finished(generation)
            };
            match decision {
                read_along::AdvanceDecision::PromotePreparedPage => {
                    let _ = app.promote_read_along_page(generation);
                }
                read_along::AdvanceDecision::PauseAfterAudio => {
                    let app_generation = app
                        .inner
                        .read_along
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_ref()
                        .map(|run| run.app_generation);
                    if let Some(app_generation) = app_generation {
                        app.finish_failed_read_along(
                            generation,
                            app_generation,
                            "retry limit reached",
                        );
                    }
                }
                read_along::AdvanceDecision::CaptureNextPage => {
                    app.start_read_along_prefetch(generation)
                }
                _ => {}
            }
        });
    }
    fn promote_read_along_page(&self, generation: u64) -> Result<()> {
        let (prepared, previous_generation, fingerprint) = {
            let mut state = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let run = state.as_mut().context("read-along is not active")?;
            anyhow::ensure!(
                run.controller.accepts(generation),
                "read-along page was superseded"
            );
            let prepared = run
                .prepared
                .take()
                .context("next page speech is not ready")?;
            let fingerprint = text_fingerprint(&prepared.text);
            (prepared, run.app_generation, fingerprint)
        };
        let app_generation = self
            .inner
            .generation
            .advance_from(previous_generation)
            .context("read-along was superseded")?;
        self.inner.audio.replace(app_generation);
        let units = plan_units(&prepared.text, 240)?;
        let session = Session {
            job_id: Uuid::new_v4().to_string(),
            generation: app_generation,
            source: "read-along".into(),
            preparation: prepared.progress.clone(),
            units,
        };
        *self
            .inner
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session);
        {
            let mut state = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let run = state.as_mut().context("read-along was stopped")?;
            run.app_generation = app_generation;
            run.active_stream = Some(prepared.playing_generation.clone());
            run.controller.promote(generation);
            run.controller.set_current_page(generation, fingerprint);
            run.preparation = read_along::Preparation::default();
            run.next_activity = None;
            run.next_progress = None;
            run.controller.request_advance(generation, false, false);
        }
        prepared
            .playing_generation
            .store(app_generation, Ordering::Release);
        let app = self.clone();
        std::thread::spawn(move || {
            let current = || app.inner.generation.accepts(app_generation);
            let mut next = Some(prepared.first);
            let mut previous: Option<(u32, u32)> = None;
            let result = (|| -> Result<()> {
                loop {
                    let chunk = match next.take() {
                        Some(chunk) => chunk,
                        None => match prepared.remaining.receive(&current)? {
                            Some(Ok(Some(chunk))) => chunk,
                            Some(Ok(None)) => break,
                            Some(Err(error)) => bail!(error),
                            None => break,
                        },
                    };
                    let (sentence, unit) = chunk_stream::tags(&chunk.unit);
                    if let Some((prior_sentence, prior_unit)) =
                        previous.filter(|(prior, _)| *prior != sentence)
                    {
                        let silence = vec![
                            0;
                            app.inner.audio.output_rate() as usize
                                * chunk.route.playback.sentence_gap_ms as usize
                                / 1000
                        ];
                        app.inner.audio.push(
                            AudioFrame {
                                generation: app_generation,
                                sentence: prior_sentence,
                                unit: prior_unit,
                                samples: &silence,
                            },
                            &chunk.route.playback,
                            current,
                        )?;
                    }
                    let samples = resample(
                        &chunk.samples,
                        media_routing::OUTPUT_RATE,
                        app.inner.audio.output_rate(),
                    );
                    app.inner.audio.push(
                        AudioFrame {
                            generation: app_generation,
                            sentence,
                            unit,
                            samples: &samples,
                        },
                        &chunk.route.playback,
                        current,
                    )?;
                    previous = Some((sentence, unit));
                }
                Ok(())
            })();
            if current() {
                app.inner.audio.finish(app_generation);
                if let Some(run) = app
                    .inner
                    .read_along
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_mut()
                    .filter(|run| run.app_generation == app_generation)
                {
                    run.active_stream = None;
                }
                if let Err(error) = result {
                    app.read_along_failed(generation, app_generation, error);
                }
            }
        });
        self.inner
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set(app_generation, "playing");
        self.show_hud();
        let prefetch = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|run| run.settings.prefetch);
        if prefetch {
            self.start_read_along_prefetch(generation);
        }
        self.wait_for_read_along_audio(generation);
        Ok(())
    }
    fn action_prepare(&self) -> Result<String> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_idle_lease();
        drop(intent);
        drop(lifecycle);
        let _lease = IdleLeaseOnDrop(self);
        let saved = self.inner.store.load()?;
        let language =
            if saved.speech.selection.mode == reader_core::routing::SelectionMode::FollowKeyboard {
                query_keyboard_language().ok()
            } else {
                None
            };
        let config = reading_hint::prewarm(&saved, language.as_deref())?;
        self.show_hud();
        self.inner.tts.prepare(&config)?;
        Ok("Preparing local voice and capture".into())
    }
    fn action_pause(&self) -> Result<String> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = self
            .inner
            .session
            .lock()
            .unwrap()
            .as_ref()
            .map(|session| session.generation)
            .context("nothing is being read")?;
        self.inner.audio.pause(true, generation);
        {
            let mut activity = self.inner.activity.lock().unwrap();
            if activity.generation == generation && activity.phase != "paused" {
                activity.before_pause = activity.phase;
                activity.set(generation, "paused");
            }
        }
        let lease = self.inner.pause_lease_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.schedule_paused_release(generation, lease);
        drop(lifecycle);
        Ok("Reading paused".into())
    }
    fn action_resume(&self) -> Result<String> {
        if self.failed_read_along_area_is_saved() {
            return self.action_repeat();
        }

        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.pause_lease_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.pause_timer.cancel();
        let generation = self.inner.generation.current();
        self.inner.audio.pause(false, generation);
        {
            let mut activity = self.inner.activity.lock().unwrap();
            let resumed = activity.before_pause;
            activity.set(generation, resumed);
        }
        let streaming = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|run| {
                run.active_stream
                    .as_ref()
                    .is_some_and(|stream| stream.load(Ordering::Acquire) == generation)
            });
        if self
            .inner
            .paused_restart_generation
            .compare_exchange(generation, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && !streaming
        {
            let session = self
                .inner
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .context("paused reading session expired")?;
            let config = self.inner.store.load()?.routed(
                reader_core::routing::Feature::Tts,
                self.inner.route_language.lock().unwrap().as_deref(),
            )?;
            let sentence = self.inner.audio.cursor().0;
            let app = self.clone();
            drop(lifecycle);
            std::thread::spawn(move || app.run_session(config, session, sentence));
        } else {
            drop(lifecycle);
        }
        let retry = {
            let mut state = self
                .inner
                .read_along
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .as_mut()
                .is_some_and(|run| run.controller.retry_failed(run.controller.generation()))
        };
        if retry {
            self.set_notice(String::new(), Duration::ZERO);
            self.action_read_along_next()?;
        }
        Ok("Reading resumed".into())
    }
    fn action_stop(&self) -> Result<String> {
        self.inner.read_along_active.store(false, Ordering::Release);
        {
            let _intent = self
                .inner
                .intent_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.inner.copy_epoch.fetch_add(1, Ordering::AcqRel);
            self.inner.speculative_epoch.fetch_add(1, Ordering::AcqRel);
            self.inner.capture_epoch.fetch_add(1, Ordering::AcqRel);
            let mut worker = self
                .inner
                .ocr_kill
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((identity, true)) = worker.as_ref() {
                match identity.terminate("OCR copy worker") {
                    Ok(()) => *worker = None,
                    Err(error) => tracing::warn!(%error, "could not stop OCR copy worker"),
                }
            }
        }
        self.begin_read_intent()?;
        self.unload_server_workers()?;
        Ok("Reading stopped".into())
    }
    fn action_unload(&self) -> Result<String> {
        self.action_stop()?;
        self.clear_idle_lease();
        self.inner.tts.shutdown()?;
        self.unload_server_workers()?;
        self.inner.ocr_timer.cancel();
        self.inner.ocr_epoch.fetch_add(1, Ordering::AcqRel);
        self.kill_ocr()?;
        let _gate = self
            .inner
            .ocr_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner
            .ocr_worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        write_device_report("Stopped · manually unloaded", false);
        self.show_hud();
        Ok("All reading models released".into())
    }
    fn action_refresh_idle_lease(&self) -> Result<String> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_idle_lease();
        drop(intent);
        drop(lifecycle);
        self.schedule_idle_lease_if_current();
        Ok("Voice inactivity timeout applied".into())
    }
    fn action_server_start(&self) -> Result<String> {
        self.start_server()?;
        Ok("Authenticated OpenAI-compatible server started".into())
    }
    fn action_server_stop(&self) -> Result<String> {
        if let Some(stop) = self.inner.server.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed)
        }
        self.stop_server_workers()?;
        Ok("Headless server stopped".into())
    }
    fn action_navigate(&self, action: &str) -> Result<String> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.pause_lease_epoch.fetch_add(1, Ordering::AcqRel);
        self.inner.pause_timer.cancel();
        self.inner
            .paused_restart_generation
            .store(0, Ordering::Release);
        let session = self
            .inner
            .session
            .lock()
            .unwrap()
            .clone()
            .context("there is no reading session")?;
        let (current, _) = self.inner.audio.cursor();
        let max = session
            .units
            .iter()
            .map(|unit| unit.sentence_index)
            .max()
            .unwrap_or(0);
        let sentence = match action {
            "replay" => current,
            "next" => current.saturating_add(1).min(max),
            _ => current.saturating_sub(1),
        };
        let config = self.inner.store.load()?.routed(
            reader_core::routing::Feature::Tts,
            self.inner.route_language.lock().unwrap().as_deref(),
        )?;
        let intent = self
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = self.inner.generation.advance();
        drop(intent);
        self.inner.audio.replace(generation);
        self.inner.tts.kill_current()?;
        let mut next = session;
        next.generation = generation;
        next.job_id = Uuid::new_v4().to_string();
        *self.inner.session.lock().unwrap() = Some(next.clone());
        drop(lifecycle);
        let app = self.clone();
        std::thread::spawn(move || app.run_session(config, next, sentence));
        let description = match action {
            "replay" => "Replaying",
            "next" => "Moving to",
            _ => "Returning to",
        };
        Ok(format!("{description} sentence {}", sentence + 1))
    }
    fn start_server(&self) -> Result<()> {
        let mut active = self.inner.server.lock().unwrap();
        if active.is_some() {
            return Ok(());
        };
        let config = self.inner.store.load()?;
        anyhow::ensure!(
            config.server.enabled,
            "enable server mode in settings before starting it"
        );
        let host = config
            .server
            .listen
            .parse::<std::net::SocketAddr>()
            .context("server listen address must be host:port")?;
        if !config.server.allow_remote && !host.ip().is_loopback() {
            bail!(
                "remote server binding is disabled; select a loopback address or enable private-network access"
            )
        }
        let token = if let Ok(value) = std::env::var("SIMPLE_TTS_SERVER_TOKEN") {
            anyhow::ensure!(
                !value.trim().is_empty() && value.len() <= 512,
                "SIMPLE_TTS_SERVER_TOKEN must contain 1 to 512 non-whitespace bytes"
            );
            value
        } else {
            match reader_core::secrets::server_key()? {
                Some(value) => value,
                None => {
                    let value = Uuid::new_v4().to_string() + &Uuid::new_v4().to_string();
                    reader_core::secrets::set_server_key(&value)?;
                    value
                }
            }
        };
        let server = tiny_http::Server::http(host)
            .map_err(|e| anyhow::anyhow!("binding server listener: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        *active = Some(stop.clone());
        let app = self.clone();
        let max = config.server.max_clients.max(1) as usize;
        std::thread::spawn(move || {
            let clients = Arc::new(AtomicUsize::new(0));
            while !stop.load(Ordering::Relaxed) {
                let Some(request) = server
                    .recv_timeout(Duration::from_millis(250))
                    .ok()
                    .flatten()
                else {
                    continue;
                };
                if clients.fetch_add(1, Ordering::Relaxed) >= max {
                    clients.fetch_sub(1, Ordering::Relaxed);
                    let _=request.respond(server_json(429,&serde_json::json!({"error":{"message":"server client limit reached","type":"rate_limit_error"}})));
                    continue;
                }
                let app = app.clone();
                let token = token.clone();
                let clients = clients.clone();
                std::thread::spawn(move || {
                    if let Err(error) = handle_server_request(&app, &token, request) {
                        tracing::warn!(%error,"headless API request failed");
                    }
                    clients.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
        Ok(())
    }
    fn kill_ocr(&self) -> Result<()> {
        let mut guard = self
            .inner
            .ocr_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((identity, false)) = guard.as_ref() {
            identity.terminate("OCR worker")?;
            *guard = None;
        }
        Ok(())
    }
    fn stop_server_workers(&self) -> Result<()> {
        if let Some(stop) = self
            .inner
            .server
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            stop.store(true, Ordering::Release);
        }
        self.unload_server_workers()
    }
    fn unload_server_workers(&self) -> Result<()> {
        self.inner
            .server_unload_epoch
            .fetch_add(1, Ordering::AcqRel);
        let jobs: Vec<_> = self
            .inner
            .server_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for job in jobs {
            job.lifetime.cancel();
            let cancelled = {
                let mut state = job
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if matches!(state.as_str(), "queued" | "running") {
                    *state = "cancelled".into();
                    true
                } else {
                    false
                }
            };
            if cancelled {
                push_job_event(&job, "cancelled", serde_json::json!({}));
            }
        }
        self.inner
            .warmups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        // Server jobs observe their own cancellation state at utterance
        // boundaries. Never terminate the shared TTS child here: it may be
        // producing desktop playback for a different owner.
        let _gate = self
            .inner
            .server_job_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if desktop_tts_in_use(&self.inner) {
            return Ok(());
        }
        self.inner.tts.shutdown()
    }
    fn kill_capture(&self) -> Result<()> {
        let mut guard = self
            .inner
            .capture_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(identity) = guard.as_ref() {
            identity.terminate("screen capture")?;
        }
        *guard = None;
        Ok(())
    }
    fn kill_capture_if(&self, incarnation: &str) -> Result<()> {
        let mut guard = self
            .inner
            .capture_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard
            .as_ref()
            .is_some_and(|identity| identity.incarnation == incarnation)
        {
            if let Some(identity) = guard.as_ref() {
                identity.terminate("selection helper")?;
            }
            *guard = None;
        }
        Ok(())
    }
    #[cfg(windows)]
    fn publish_capture(&self, generation: u64, identity: ChildKill, label: &str) -> Result<()> {
        let lifecycle = self
            .inner
            .pause_lifecycle_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut capture = self
            .inner
            .capture_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.inner.generation.accepts(generation) || capture.is_some() {
            drop(capture);
            drop(lifecycle);
            identity.terminate(label)?;
            bail!("{label} was canceled or superseded");
        }
        *capture = Some(identity);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    fn kill_clipboard_owner(&self) -> Result<()> {
        let mut owner = self
            .inner
            .clipboard_owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((identity, mut child)) = owner.take() {
            identity.terminate("temporary clipboard owner")?;
            let _ = child.wait();
        }
        Ok(())
    }
    fn status(&self) -> serde_json::Value {
        let session = self
            .inner
            .session
            .lock()
            .unwrap()
            .clone()
            .filter(|session| self.inner.generation.accepts(session.generation));
        let (sentence, unit) = self.inner.audio.cursor();
        let sentence_text = session
            .as_ref()
            .map(|s| {
                s.units
                    .iter()
                    .filter(|u| u.sentence_index == sentence)
                    .map(|u| u.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let total = session
            .as_ref()
            .map(|s| s.units.iter().map(|u| u.sentence_index).max().unwrap_or(0) + 1)
            .unwrap_or(0);
        let chunk = session
            .as_ref()
            .and_then(|s| chunk_progress(&s.units, sentence, unit));
        let chunk_index = chunk.as_ref().map(|progress| progress.index).unwrap_or(0);
        let chunk_total = chunk.as_ref().map(|progress| progress.total).unwrap_or(0);
        let chunk_text = chunk
            .as_ref()
            .map(|progress| progress.text.as_str())
            .unwrap_or_default();
        let auto_dismiss_seconds = self
            .inner
            .store
            .load()
            .map(|config| config.playback.auto_dismiss_seconds)
            .unwrap_or(2);
        let warm_timeout_seconds = self
            .inner
            .store
            .load()
            .map(|config| config.playback.keep_warm_seconds)
            .unwrap_or(300);
        let idle_unload_remaining_seconds = if self.idle_worker_incarnation().is_some() {
            self.inner
                .idle_deadline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
                        .ceil() as u64
                })
        } else {
            None
        };
        let model_lifecycle = self.inner.tts.model_lifecycle();
        let (notice_text, notice_revision, notice_expires_in_ms) = {
            let notice = self
                .inner
                .notice
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let remaining = notice
                .expires
                .map(|expires| expires.saturating_duration_since(Instant::now()));
            let text = if remaining.is_some_and(|value| value.is_zero()) {
                String::new()
            } else {
                notice.text.clone()
            };
            (
                text,
                notice.revision,
                remaining.map(|value| value.as_millis().min(u64::MAX as u128) as u64),
            )
        };
        let device = self.inner.tts.published_device();
        let paused = self
            .inner
            .audio
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .paused;
        let phase = if session.is_none() {
            "idle"
        } else if paused {
            "paused"
        } else if device.is_none() {
            "preparing"
        } else {
            "reading"
        };
        let activity = self.inner.activity.lock().unwrap().clone();
        let (read_along_active, next_page_activity) = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|run| (true, run.next_activity.as_ref().map(|activity| serde_json::json!({"phase": activity.phase, "wait_remaining_ms": activity.waiting_until.map(|deadline| deadline.saturating_duration_since(Instant::now()).as_millis() as u64).unwrap_or(0), "generated_chunks": run.next_progress.as_ref().map(|p| p.generated()).unwrap_or(0), "chunk_total": run.next_progress.as_ref().map(|p| p.total()).unwrap_or(0)}))))
            .unwrap_or((self.inner.read_along_active.load(Ordering::Acquire), None));
        let prepared_text = session
            .as_ref()
            .filter(|s| {
                s.generation == activity.generation && self.inner.generation.accepts(s.generation)
            })
            .and_then(|_| activity.prepared_text.clone());
        let ocr_activity = self.inner.ocr_activity.lock().unwrap().clone();
        let (last_action_at_ms, last_action_kind) = self
            .inner
            .repeat_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .metadata();
        serde_json::json!({"activity":activity,"ocr_activity":ocr_activity,"read_along_active":read_along_active,"next_page_activity":next_page_activity,"controller_online":true,"state":phase,"last_action_at_ms":last_action_at_ms,"last_action_kind":last_action_kind,"job_id":session.as_ref().map(|s|s.job_id.clone()),"generation":session.as_ref().map(|s|s.generation),"read_pending":self.inner.read_pending.load(Ordering::Acquire),"clipboard_listening":self.inner.clipboard_listening.load(Ordering::Acquire),"keep_warm_seconds":warm_timeout_seconds,"idle_unload_remaining_seconds":idle_unload_remaining_seconds,"sentence":sentence,"sentence_text":sentence_text,"sentence_total":total,"unit":unit,"chunk":chunk_index,"chunk_total":chunk_total,"chunks_ready_ahead":session.as_ref().map(|s| s.preparation.ready_after(chunk_index)).unwrap_or(0),"chunks_generated":session.as_ref().map(|s| s.preparation.generated()).unwrap_or(0),"chunk_text":chunk_text,"prepared_text":prepared_text,"model_events":self.inner.model_events.snapshot(),"residency":{"tts":self.inner.tts.residency.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),"ocr":self.inner.ocr_residency.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()},"model_status":model_lifecycle.status,"model_status_revision":model_lifecycle.revision,"notice_text":notice_text,"notice_revision":notice_revision,"notice_expires_in_ms":notice_expires_in_ms,"source":session.map(|s|s.source),"device":device.unwrap_or_else(||if paused{"Voice worker released during pause".into()}else{"Voice model is preparing".into()}),"route":if matches!(self.inner.store.load().map(|c|c.speech.provider),Ok(SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic)){"local"}else{"remote"},"auto_dismiss_seconds":auto_dismiss_seconds,"audio_queued_samples":self.inner.audio.queued(),"audio_callback_buffers":self.inner.audio.inner.callback_buffers.load(Ordering::Relaxed),"audio_callback_frames":self.inner.audio.inner.callback_frames.load(Ordering::Relaxed),"audio_callback_contention":self.inner.audio.inner.callback_contention.load(Ordering::Relaxed),"audio_callback_errors":self.inner.audio.inner.callback_errors.load(Ordering::Relaxed),"audio_error":self.inner.audio.inner.fatal_error.lock().unwrap().clone()})
    }
    fn read_image(&self, path: &str) -> Result<String> {
        let generation = self.begin_read_intent_with_pending(true)?;
        let result = self.read_image_at(path, generation);
        if let Err(error) = &result {
            self.report_intent_failure(generation, "Screen read failed", error, false);
        }
        result
    }
    fn read_image_at(&self, path: &str, generation: u64) -> Result<String> {
        self.read_image_at_source(Path::new(path), generation, "screen region")
    }
    fn read_image_at_source(&self, image: &Path, generation: u64, source: &str) -> Result<String> {
        let recognized = self.recognize_image_at_source(image, generation)?;
        self.start_text_at(source.into(), recognized, generation)
    }
    fn recognize_image_at_source(&self, image: &Path, generation: u64) -> Result<String> {
        anyhow::ensure!(
            self.inner.generation.accepts(generation),
            "OCR request was canceled"
        );
        // End the picker-only prewarm phase before the real OCR can queue.
        // An older generation must never invalidate a newer selection.
        end_ocr_prewarm_phase(
            &self.inner.intent_gate,
            &self.inner.ocr_prewarm_epoch,
            || self.inner.generation.accepts(generation),
        )?;
        let config = self.inner.store.load()?.routed(
            reader_core::routing::Feature::Ocr,
            self.inner.route_language.lock().unwrap().as_deref(),
        )?;
        let background = self
            .inner
            .read_along
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|run| run.app_generation == generation);
        if !background {
            let mut activity = self.inner.activity.lock().unwrap();
            if activity.generation == generation {
                activity.language = Some(config.ocr.language.clone());
                activity.model = config
                    .ocr
                    .model_dir
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.runtime = config
                    .ocr
                    .runtime_dir
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.set(generation, "recognizing");
            }
        }
        let recognized = run_ocr(
            self,
            &config,
            image,
            20_000_000,
            false,
            || self.inner.generation.accepts(generation),
            None,
        )?;
        if !self.inner.generation.accepts(generation) {
            bail!("OCR request was canceled")
        };
        Ok(recognized)
    }
    fn recognize_read_along_image(&self, image: &Path, generation: u64) -> Result<String> {
        let result = self.recognize_image_at_source(image, generation);
        if let Err(error) = std::fs::remove_file(image) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(%error, "could not remove read-along capture image");
            }
        }
        result
    }
    fn recognize_read_along_page(
        &self,
        target: &read_along_capture::Target,
        image: PathBuf,
        generation: u64,
    ) -> Result<String> {
        let current = || self.inner.generation.accepts(generation);
        let mut image = image;
        for attempt in 0..3 {
            let result = self.recognize_read_along_image(&image, generation);
            let empty = match &result {
                Ok(text) => text.trim().is_empty(),
                Err(error) => error.chain().any(|cause| {
                    cause
                        .to_string()
                        .to_ascii_lowercase()
                        .contains("no readable text")
                }),
            };
            if !empty || attempt == 2 {
                return result;
            }
            // Retry the same page during a transient animation or blank frame.
            // Only the outer unchanged-page retry is allowed to send another key.
            for _ in 0..5 {
                anyhow::ensure!(current(), "read-along OCR canceled");
                std::thread::sleep(Duration::from_millis(100));
            }
            image = target.capture(&current)?;
        }
        unreachable!()
    }
    fn handle(&self, request: LocalRequest) -> Result<LocalReply> {
        privacy::set_content_logging(self.inner.store.load()?.privacy.log_content);
        match request {
            LocalRequest::ReadStart { id, source } => {
                let generation = self.begin_read_intent_with_pending(true)?;
                let lifecycle = self
                    .inner
                    .pause_lifecycle_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                anyhow::ensure!(
                    self.inner.generation.accepts(generation),
                    "reading request was superseded"
                );
                let mut drafts = self.inner.drafts.lock().unwrap();
                drafts.retain(|_, draft| draft.created.elapsed() < Duration::from_secs(60));
                if drafts.len() >= 32 {
                    bail!("too many unfinished reading requests")
                };
                let repeat_revision = self.begin_repeat_action(&source);
                drafts.insert(
                    id.clone(),
                    Draft {
                        source,
                        text: String::new(),
                        created: Instant::now(),
                        generation,
                        repeat_revision,
                    },
                );
                drop(drafts);
                drop(lifecycle);
                let app = self.clone();
                self.inner.draft_timer.schedule(
                    Instant::now() + Duration::from_secs(60),
                    move || {
                        let intent = app
                            .inner
                            .intent_gate
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if !app.inner.generation.accepts(generation) {
                            return;
                        }
                        let removed = {
                            let mut drafts = app
                                .inner
                                .drafts
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if drafts
                                .get(&id)
                                .is_some_and(|draft| draft.generation == generation)
                            {
                                drafts.remove(&id);
                                drafts.is_empty()
                            } else {
                                false
                            }
                        };
                        if removed {
                            app.inner.read_pending.store(false, Ordering::Release);
                        }
                        drop(intent);
                        if removed {
                            app.schedule_idle_lease_if_current();
                        }
                    },
                );
                Ok(LocalReply::ok("Reading request accepted"))
            }
            LocalRequest::ReadChunk { id, text } => {
                let mut drafts = self.inner.drafts.lock().unwrap();
                let draft = drafts.get_mut(&id).context("reading request expired")?;
                anyhow::ensure!(
                    self.inner.generation.accepts(draft.generation),
                    "reading request was superseded"
                );
                if draft.text.len() + text.len() > MAX_TEXT_BYTES {
                    let generation = draft.generation;
                    drafts.remove(&id);
                    drop(drafts);
                    let error = anyhow::anyhow!("reading text exceeds the 1 MiB limit");
                    self.report_intent_failure(generation, "Reading request failed", &error, false);
                    return Err(error);
                };
                draft.text.push_str(&text);
                Ok(LocalReply::ok("Text received"))
            }
            LocalRequest::ReadFinish { id } => {
                let draft = self
                    .inner
                    .drafts
                    .lock()
                    .unwrap()
                    .remove(&id)
                    .context("reading request expired")?;
                self.inner.draft_timer.cancel();
                match self.start_text_at(draft.source.clone(), draft.text.clone(), draft.generation)
                {
                    Ok(message) => {
                        self.complete_repeat_action(
                            draft.repeat_revision,
                            repeat::RepeatPayload::Text {
                                source: draft.source,
                                text: draft.text,
                            },
                        );
                        Ok(LocalReply::ok(message))
                    }
                    Err(error) => {
                        self.report_intent_failure(
                            draft.generation,
                            "Reading request failed",
                            &error,
                            false,
                        );
                        Err(error)
                    }
                }
            }
            LocalRequest::RenderFile { input, output } => {
                let abort_epoch = self.inner.server_unload_epoch.load(Ordering::Acquire);
                anyhow::ensure!(
                    std::fs::metadata(&input)?.len() <= MAX_TEXT_BYTES as u64,
                    "Text file exceeds limit"
                );
                let text = std::fs::read_to_string(input)?;
                let saved = self.inner.store.load()?;
                let language = if saved.speech.selection.mode
                    == reader_core::routing::SelectionMode::FollowKeyboard
                {
                    query_keyboard_language().ok()
                } else {
                    None
                };
                let config = saved.routed_text(&text, language.as_deref())?;
                {
                    let _intent = self
                        .inner
                        .intent_gate
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    self.inner.speculative_epoch.fetch_add(1, Ordering::AcqRel);
                    self.inner
                        .tts
                        .resolve_speculation(self.inner.generation.current(), &config)?;
                }
                self.clear_idle_lease();
                let _lease = IdleLeaseOnDrop(self);
                let is_current = || {
                    !self.inner.shutdown.load(Ordering::Acquire)
                        && self.inner.server_unload_epoch.load(Ordering::Acquire) == abort_epoch
                };
                let (samples, last_route) = collect_routed_media(
                    &self.inner.tts,
                    config.clone(),
                    &text,
                    MediaOptions {
                        voice: None,
                        speed: config.speech.speed,
                        max_samples: media_routing::MAX_SAMPLES,
                        request_id: None,
                        lifetime: None,
                        current: &is_current,
                    },
                    |_, _, _| {},
                )?;
                anyhow::ensure!(
                    self.inner.server_unload_epoch.load(Ordering::Acquire) == abort_epoch,
                    "speech rendering was cancelled"
                );
                let rate = media_routing::OUTPUT_RATE;
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(output)?;
                let mut writer = hound::WavWriter::new(
                    file,
                    hound::WavSpec {
                        channels: 1,
                        sample_rate: rate,
                        bits_per_sample: 16,
                        sample_format: hound::SampleFormat::Int,
                    },
                )?;
                for sample in samples {
                    writer.write_sample(sample)?;
                }
                writer.finalize()?;
                if let Err(error) = reading_hint::record(&last_route) {
                    tracing::debug!(%error, "could not save last voice route");
                }
                Ok(LocalReply::ok("Audio rendered"))
            }
            LocalRequest::OcrFile { path } => {
                let generation = self.inner.generation.current();
                let source = Path::new(&path);
                anyhow::ensure!(
                    std::fs::metadata(source)?.len() <= 12 * 1024 * 1024,
                    "Image exceeds 12 MiB"
                );
                let directory = data_dir().join("capture");
                std::fs::create_dir_all(&directory)?;
                let temporary = directory.join(format!("cli-{}.png", Uuid::new_v4()));
                let _image_cleanup =
                    privacy::TemporaryFile::create(temporary.clone(), &std::fs::read(source)?)?;
                let result = (|| {
                    let saved = self.inner.store.load()?;
                    let language = if saved.ocr.selection.mode
                        == reader_core::routing::SelectionMode::FollowKeyboard
                    {
                        query_keyboard_language().ok()
                    } else {
                        None
                    };
                    let config =
                        saved.routed(reader_core::routing::Feature::Ocr, language.as_deref())?;
                    run_ocr(
                        self,
                        &config,
                        &temporary,
                        20_000_000,
                        false,
                        || self.inner.generation.accepts(generation),
                        None,
                    )
                })();
                let _ = std::fs::remove_file(temporary);
                Ok(LocalReply::ok(result?))
            }
            LocalRequest::ReadImage { path } => Ok(LocalReply::ok(self.read_image(&path)?)),
            LocalRequest::Action { action } => Ok(LocalReply::ok(self.action(&action)?)),
            #[cfg(target_os = "linux")]
            LocalRequest::Input { id, action } => {
                portal_input::local(id, action)?;
                Ok(LocalReply::ok("input delivered"))
            }
            #[cfg(target_os = "linux")]
            LocalRequest::CancelInput { id } => {
                portal_input::cancel(&id);
                Ok(LocalReply::ok("input canceled"))
            }
            LocalRequest::Status => Ok(LocalReply::state(self.status())),
            LocalRequest::Shutdown => {
                self.inner
                    .clipboard_listening
                    .store(false, Ordering::Release);
                self.inner
                    .clipboard_listener_epoch
                    .fetch_add(1, Ordering::AcqRel);
                self.stop_clipboard_listener();
                let lifecycle = self
                    .inner
                    .pause_lifecycle_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                self.inner.pause_lease_epoch.fetch_add(1, Ordering::AcqRel);
                self.inner.pause_timer.cancel();
                self.inner
                    .paused_restart_generation
                    .store(0, Ordering::Release);
                let intent = self
                    .inner
                    .intent_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let generation = self.inner.generation.advance();
                drop(intent);
                self.inner.audio.stop(generation);
                self.kill_capture()?;
                self.kill_ocr()?;
                self.inner.tts.shutdown()?;
                self.stop_server_workers()?;
                drop(lifecycle);
                Ok(LocalReply::ok("Controller shutting down"))
            }
        }
    }
}

fn store_clipboard_image(bytes: &[u8], extension: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES,
        "clipboard image exceeds its size limit"
    );
    let extension = match extension {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "png",
        "jpg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => "jpg",
        "webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => "webp",
        _ => bail!("clipboard image format does not match its data"),
    };
    let directory = data_dir().join("capture");
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = directory.join(format!("clipboard-{}.{}", Uuid::new_v4(), extension));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&path)?.write_all(bytes)?;
    Ok(path)
}

struct OcrWorker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    fingerprint: String,
    events: model_events::ModelEvents,
    model_id: String,
    runtime_id: String,
    language: String,
    native: bool,
    loaded: bool,
}
impl Drop for OcrWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if self.native {
            self.events.record(
                "ocr",
                &self.model_id,
                &self.runtime_id,
                &self.language,
                if self.loaded {
                    "unloaded"
                } else {
                    "loading_canceled"
                },
            );
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum OcrCacheDecision {
    PreserveForCapture,
    RetainUntilIdle,
}

fn ocr_cache_decision(preserve_for_capture: bool) -> OcrCacheDecision {
    if preserve_for_capture {
        OcrCacheDecision::PreserveForCapture
    } else {
        OcrCacheDecision::RetainUntilIdle
    }
}

fn with_ocr_worker<T>(
    app: &App,
    config: &AppConfig,
    max_image_pixels: u32,
    is_copy: bool,
    prewarm: (bool, Option<&AtomicU64>),
    current: impl Fn() -> bool,
    operation: impl FnOnce(&mut OcrWorker) -> Result<T>,
) -> Result<T> {
    let (preserve_for_capture, prewarm_marker) = prewarm;
    let _serial = app
        .inner
        .ocr_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    anyhow::ensure!(
        current() && !app.inner.shutdown.load(Ordering::Acquire),
        "OCR request was canceled"
    );
    let capture = data_dir().join("capture");
    std::fs::create_dir_all(&capture)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&capture, std::fs::Permissions::from_mode(0o700))?;
    }
    let allowed = capture
        .canonicalize()
        .context("private capture directory is unavailable")?;
    let provider = match config.ocr.provider {
        reader_core::OcrProvider::LocalPaddle => "local_paddle",
        _ => "openai_compatible",
    };
    let device = match config.ocr.device {
        DevicePreference::Cuda => "gpu",
        DevicePreference::Cpu => "cpu",
        DevicePreference::Auto => "auto",
    };
    let fingerprint = format!(
        "{}|{}|{}|{}|{}|{:?}|{max_image_pixels}",
        serde_json::to_string(&config.ocr.provider)?,
        config.ocr.language,
        config.ocr.model_dir.display(),
        config.ocr.runtime_dir.display(),
        device,
        config.ocr.worker_library
    );
    let epoch = app.inner.ocr_epoch.fetch_add(1, Ordering::AcqRel) + 1;
    app.inner.ocr_timer.cancel();
    let mut slot = app
        .inner
        .ocr_worker
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.as_mut().is_some_and(|worker| {
        worker.fingerprint != fingerprint || !matches!(worker.child.try_wait(), Ok(None))
    }) {
        slot.take();
    }
    let mut worker = if let Some(worker) = slot.take() {
        worker
    } else {
        if provider == "local_paddle" {
            app.inner.model_events.record(
                "ocr",
                &model_events::identity(&config.ocr.model_dir),
                &model_events::identity(&config.ocr.runtime_dir),
                &config.ocr.language,
                "loading",
            );
        }
        let mut command = Command::new(sibling("reader-ocr")?);
        configure_worker_requests(&mut command, config, config.ocr.fallback_to_local);
        command
            .args(["--provider", provider, "--model-dir"])
            .arg(&config.ocr.model_dir)
            .arg("--runtime-dir")
            .arg(&config.ocr.runtime_dir)
            .args(["--device", device, "--max-image-pixels"])
            .arg(max_image_pixels.to_string());
        if let reader_core::OcrProvider::OpenAiCompatible {
            base_url,
            model,
            api,
        } = &config.ocr.provider
        {
            command.args([
                "--base-url",
                base_url,
                "--model",
                model,
                "--api",
                if *api == reader_core::config::OcrApi::Responses {
                    "responses"
                } else {
                    "chat_completions"
                },
            ]);
        }
        if let Some(library) = &config.ocr.worker_library {
            command.arg("--library").arg(library);
        }
        let paths = reader_core::native_paths::library_search_path(
            &config.ocr.runtime_dir,
            std::env::var_os("LD_LIBRARY_PATH").as_deref(),
        )?;
        command
            .env("LD_LIBRARY_PATH", paths)
            .env("SIMPLE_TTS_CAPTURE_DIR", &allowed)
            .env(
                "SIMPLE_TTS_OCR_API_KEY",
                reader_core::secrets::ocr_key()?.unwrap_or_default(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().context("starting OCR worker")?;
        let input = child.stdin.take().context("OCR worker stdin missing")?;
        let output = BufReader::new(child.stdout.take().context("OCR worker stdout missing")?);
        OcrWorker {
            child,
            input,
            output,
            fingerprint,
            events: app.inner.model_events.clone(),
            model_id: model_events::identity(&config.ocr.model_dir),
            runtime_id: model_events::identity(&config.ocr.runtime_dir),
            language: config.ocr.language.clone(),
            native: provider == "local_paddle",
            loaded: false,
        }
    };
    *app.inner.ocr_kill.lock().unwrap() = Some((
        ChildKill::new(worker.child.id(), Uuid::new_v4().to_string())?,
        is_copy,
    ));
    let response = if current() {
        operation(&mut worker)
    } else {
        Err(anyhow::anyhow!("OCR request was canceled"))
    };
    *app.inner.ocr_kill.lock().unwrap() = None;
    // Abort and the final publication share the same gate; no worker from a
    // canceled selection can be cached after Abort has finished cleanup.
    commit_current_ocr(&app.inner.intent_gate, current, || {
        let value = response?;
        if worker.native && !worker.loaded {
            worker.events.record(
                "ocr",
                &worker.model_id,
                &worker.runtime_id,
                &worker.language,
                "loaded",
            );
            worker.loaded = true;
        }
        match ocr_cache_decision(preserve_for_capture) {
            OcrCacheDecision::PreserveForCapture => {
                *slot = Some(worker);
                if let Some(marker) = prewarm_marker {
                    marker.store(epoch, Ordering::Release);
                }
            }
            OcrCacheDecision::RetainUntilIdle => {
                *slot = Some(worker);
                arm_ocr_idle(&app.inner, epoch, config.ocr.idle_timeout_secs);
            }
        }
        // Let Abort acquire OCR locks before releasing the intent gate. This
        // prevents it from missing a newly published prewarm.
        drop(slot);
        drop(_serial);
        Ok(value)
    })
}

// Caller holds the OCR gate when scheduling so a newer operation cannot have
// its timer replaced by an older prewarm's cleanup.
fn arm_ocr_idle(inner: &Arc<AppInner>, epoch: u64, seconds: u16) {
    let inner = Arc::clone(inner);
    let timer_inner = Arc::clone(&inner);
    inner.ocr_timer.schedule(
        Instant::now() + Duration::from_secs(u64::from(seconds)),
        move || {
            if timer_inner.ocr_epoch.load(Ordering::Acquire) == epoch {
                let mut slot = timer_inner.ocr_worker.lock().unwrap();
                if timer_inner.ocr_epoch.load(Ordering::Acquire) == epoch {
                    slot.take();
                }
            }
        },
    );
}

#[cfg(any(target_os = "linux", windows))]
fn finish_copy_prewarm(
    inner: &Arc<AppInner>,
    active: &AtomicBool,
    marker: &Arc<AtomicU64>,
    seconds: u16,
) {
    // Publication and cancellation share this gate: after it is cleared, an
    // unfinished prewarm cannot publish a worker without an idle lease.
    {
        let _intent = inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        active.store(false, Ordering::Release);
    }
    let inner = Arc::clone(inner);
    let marker = Arc::clone(marker);
    std::thread::spawn(move || {
        let _serial = inner
            .ocr_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let epoch = marker.load(Ordering::Acquire);
        if copy_prewarm_should_arm(epoch, inner.ocr_epoch.load(Ordering::Acquire)) {
            arm_ocr_idle(&inner, epoch, seconds);
        }
    });
}

#[cfg(any(target_os = "linux", windows))]
fn copy_prewarm_should_arm(published: u64, current: u64) -> bool {
    published != 0 && published == current
}

fn prepare_ocr(
    app: &App,
    config: &AppConfig,
    max_image_pixels: u32,
    is_copy: bool,
    preserve_for_capture: bool,
    prewarm_marker: Option<&AtomicU64>,
    current: impl Fn() -> bool,
) -> Result<()> {
    with_ocr_worker(
        app,
        config,
        max_image_pixels,
        is_copy,
        (preserve_for_capture, prewarm_marker),
        current,
        |worker| {
            write_json_frame(
                &mut worker.input,
                &WorkerRequest::Prepare {
                    device: device_name(config.ocr.device).into(),
                },
            )?;
            loop {
                match read_worker_frame::<WorkerEvent>(&mut worker.output)? {
                    WorkerFrame::Control(WorkerEvent::Ready { .. }) => return Ok(()),
                    WorkerFrame::Control(WorkerEvent::Error { message, .. }) => {
                        bail!("OCR preparation failed: {message}")
                    }
                    WorkerFrame::Pcm { .. } => bail!("unexpected PCM during OCR preparation"),
                    _ => {}
                }
            }
        },
    )
}

fn run_ocr(
    app: &App,
    config: &AppConfig,
    image: &Path,
    max_image_pixels: u32,
    is_copy: bool,
    current: impl Fn() -> bool,
    lifetime: Option<&request_lifetime::RequestLifetime>,
) -> Result<String> {
    let allowed = data_dir()
        .join("capture")
        .canonicalize()
        .context("private capture directory is unavailable")?;
    let image = image
        .canonicalize()
        .context("capture image is unavailable")?;
    if !image.starts_with(&allowed) {
        bail!("image path is outside the app-private capture directory")
    }
    with_ocr_worker(
        app,
        config,
        max_image_pixels,
        is_copy,
        (false, None),
        || current() && !app.inner.shutdown.load(Ordering::Acquire),
        |worker| {
            if let Some(lifetime) = lifetime {
                lifetime.check()?;
            }
            let job = Uuid::new_v4().to_string();
            write_json_frame(
                &mut worker.input,
                &WorkerRequest::Recognize {
                    job_id: job.clone(),
                    generation: 1,
                    image_path: image.to_string_lossy().into_owned(),
                    language: config.ocr.language.clone(),
                },
            )?;
            let mut text = None;
            loop {
                if let Some(lifetime) = lifetime {
                    lifetime.check()?;
                }
                match read_worker_frame::<WorkerEvent>(&mut worker.output)? {
                    WorkerFrame::Control(WorkerEvent::Ready { .. }) => {
                        if let Some(lifetime) = lifetime {
                            lifetime
                                .set_stage(reader_core::request_budget::RequestStage::Inference);
                        }
                    }
                    WorkerFrame::Control(WorkerEvent::Text {
                        job_id,
                        generation: 1,
                        text: recognized,
                    }) if job_id == job => text = Some(recognized),
                    WorkerFrame::Control(WorkerEvent::Finished {
                        job_id,
                        generation: 1,
                    }) if job_id == job => break,
                    WorkerFrame::Control(WorkerEvent::Error { message, .. }) => {
                        bail!("OCR failed: {message}")
                    }
                    _ => {}
                }
            }
            text.context("OCR worker returned no text")
        },
    )
    .inspect(|text| privacy::text_metadata("ocr", text))
}

fn push_job_event(job: &ServerJob, kind: &str, detail: serde_json::Value) {
    let mut events = job
        .events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sequence = next_job_event_sequence(&events);
    events.push(serde_json::json!({"job_id":job.id,"generation":job.generation,"sequence":sequence,"type":kind,"created_ms":job.created.elapsed().as_millis(),"data":detail}));
    if events.len() > 128 {
        events.remove(0);
    }
}

fn next_job_event_sequence(events: &[serde_json::Value]) -> u64 {
    events
        .last()
        .and_then(|event| event.get("sequence"))
        .and_then(serde_json::Value::as_u64)
        .map_or(0, |sequence| sequence.saturating_add(1))
}

fn ocr_server_image(
    app: &App,
    bytes: &[u8],
    unload_epoch: u64,
    lifetime: &request_lifetime::RequestLifetime,
) -> Result<String> {
    anyhow::ensure!(
        bytes.len() <= 12 * 1024 * 1024,
        "image exceeds the 12 MiB limit"
    );
    let capture = data_dir().join("capture");
    std::fs::create_dir_all(&capture)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&capture, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = capture.join(format!("server-{}.img", Uuid::new_v4()));
    let _image_cleanup = privacy::TemporaryFile::create(path.clone(), bytes)?;
    let saved = app.inner.store.load()?;
    let keyboard =
        if saved.ocr.selection.mode == reader_core::routing::SelectionMode::FollowKeyboard {
            query_keyboard_language().ok()
        } else {
            None
        };
    let config = saved.routed(reader_core::routing::Feature::Ocr, keyboard.as_deref())?;
    lifetime.set_stage(reader_core::request_budget::RequestStage::LoadingModel);
    let result = run_ocr(
        app,
        &config,
        &path,
        config.server.max_image_pixels,
        false,
        || {
            app.inner.server_unload_epoch.load(Ordering::Acquire) == unload_epoch
                && lifetime.check().is_ok()
        },
        Some(lifetime),
    );
    let _ = std::fs::remove_file(path);
    result
}

struct MediaOptions<'a> {
    voice: Option<&'a str>,
    speed: f32,
    max_samples: usize,
    request_id: Option<&'a str>,
    lifetime: Option<&'a request_lifetime::RequestLifetime>,
    current: &'a dyn Fn() -> bool,
}

struct SynthesisOptions<'a> {
    priority: bool,
    starts_in_code: bool,
    voice: &'a str,
    speed: f32,
    request_id: Option<&'a str>,
    lifetime: Option<&'a request_lifetime::RequestLifetime>,
    current: &'a dyn Fn() -> bool,
}

fn collect_routed_media(
    worker: &Arc<WorkerSupervisor>,
    config: AppConfig,
    text: &str,
    options: MediaOptions<'_>,
    mut on_unit: impl FnMut(&SpeechUnit, usize, usize),
) -> Result<(Vec<i16>, AppConfig)> {
    privacy::text_metadata("tts", text);
    let mut route = config;
    let mut samples = Vec::new();
    for unit in plan_units(text, 240)? {
        anyhow::ensure!((options.current)(), "speech rendering was cancelled");
        route = media_routing::unit_route(&route, &unit.text)?;
        if prepare_speech_text(&route, &unit.text, unit.starts_in_code)
            .trim()
            .is_empty()
        {
            continue;
        }
        let (pcm, rate) = worker.collect(
            &route,
            &unit.text,
            SynthesisOptions {
                priority: false,
                starts_in_code: unit.starts_in_code,
                voice: options.voice.unwrap_or(&route.speech.voice),
                speed: options.speed,
                request_id: options.request_id,
                lifetime: options.lifetime,
                current: options.current,
            },
        )?;
        anyhow::ensure!((options.current)(), "speech rendering was cancelled");
        let start = samples.len();
        media_routing::append_pcm(&mut samples, &pcm, rate, options.max_samples)?;
        on_unit(&unit, start, samples.len() - start);
    }
    Ok((samples, route))
}

fn server_tts_models() -> Vec<serde_json::Value> {
    let mut models = vec![
        serde_json::json!({"id":"kokoro-v1","object":"model","owned_by":"simple-tts","capabilities":["tts"]}),
    ];
    let assets = reader_core::config::data_dir()
        .parent()
        .unwrap()
        .join("assets");
    if reader_core::routing::asset_installed("supertonic3", &assets) {
        models.push(serde_json::json!({"id":"supertonic3","object":"model","owned_by":"simple-tts","capabilities":["tts"]}));
    }
    models
}
// The selected model fixes a monolingual route. Re-detecting tiny client chunks
// otherwise changes an English request into an unsupported language mid-page.
fn server_speech_language(languages: &[String], input: &serde_json::Value, text: &str) -> String {
    input["language"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if languages.len() == 1 {
                languages[0].clone()
            } else {
                reader_core::language::resolve(text)
            }
        })
}
fn server_tts_route(
    config: &AppConfig,
    input: &serde_json::Value,
    text: &str,
) -> Result<AppConfig> {
    let available = server_tts_models();
    let allowed: Vec<_> = available
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect();
    let requested = requested_server_model(input, "kokoro-v1", &allowed)?;
    let model = if requested == "supertonic3" {
        "supertonic3"
    } else {
        "kokoro"
    };
    let registry = reader_core::routing::registry();
    let entry = registry
        .models
        .iter()
        .find(|entry| entry.id == model)
        .context("unknown speech model")?;
    let assets = reader_core::config::data_dir()
        .parent()
        .context("missing asset directory")?
        .join("assets");
    let existing = std::iter::once(&config.speech.selection.single)
        .chain(config.speech.selection.languages.values())
        .find(|binding| {
            binding.model.as_deref() == Some(model)
                && binding.runtime.as_ref().is_some_and(|id| {
                    registry.runtimes.iter().any(|runtime| {
                        runtime.id == *id
                            && reader_core::routing::runtime_installed(runtime, &assets)
                    })
                })
        });
    let mut binding = existing.cloned().unwrap_or_default();
    if binding.runtime.is_none() {
        binding.runtime = registry
            .runtimes
            .iter()
            .find(|runtime| {
                runtime.feature == reader_core::routing::Feature::Tts
                    && runtime.engine == entry.engine
                    && runtime.backend == "cpu"
                    && reader_core::routing::runtime_installed(runtime, &assets)
            })
            .map(|runtime| runtime.id.clone());
    }
    binding.model = Some(model.into());
    binding.remote = None;
    let mut selected = config.clone();
    selected.speech.provider = if model == "supertonic3" {
        SpeechProvider::LocalSupertonic
    } else {
        SpeechProvider::LocalKokoro
    };
    selected.speech.selection.mode = reader_core::routing::SelectionMode::SingleModel;
    selected.speech.selection.single = binding;
    selected.speech.language = server_speech_language(&entry.languages, input, text);
    selected.routed_text(text, None)
}

fn routed_media_config(config: &AppConfig, text: &str) -> Result<AppConfig> {
    let keyboard =
        if config.speech.selection.mode == reader_core::routing::SelectionMode::FollowKeyboard {
            query_keyboard_language().ok()
        } else {
            None
        };
    config.routed_text(text, keyboard.as_deref())
}

fn server_tts_in_use(inner: &Arc<AppInner>) -> bool {
    let now = Instant::now();
    let has_leases = {
        let mut leases = inner
            .warmups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        leases.retain(|_, (_, expires)| *expires > now);
        !leases.is_empty()
    };
    let active = inner
        .active_server_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    let queued = inner
        .server_jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .any(|job| {
            matches!(
                job.state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_str(),
                "queued" | "running"
            )
        });
    has_leases || active || queued
}

fn server_worker_in_use(inner: &Arc<AppInner>) -> bool {
    server_tts_in_use(inner) || desktop_tts_in_use(inner)
}

fn desktop_tts_in_use(inner: &Arc<AppInner>) -> bool {
    let has_session = inner
        .session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    has_session || !audio_is_idle(&inner.audio)
}

// Call only while `server_job_gate` is held. This serializes retirement with
// compatibility requests, warmup, and job transitions.
fn retire_server_worker_if_idle_locked(inner: &Arc<AppInner>) -> Result<bool> {
    let Some(incarnation) = inner.tts.incarnation() else {
        return Ok(false);
    };
    if server_worker_in_use(inner) {
        return Ok(false);
    }
    let seconds = inner.store.load()?.playback.keep_warm_seconds;
    let epoch = inner.server_idle_epoch.fetch_add(1, Ordering::AcqRel) + 1;
    let weak = Arc::downgrade(inner);
    inner.server_idle_timer.schedule(
        Instant::now() + Duration::from_secs(u64::from(seconds)),
        move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let _gate = inner
                .server_job_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inner.server_idle_epoch.load(Ordering::Acquire) == epoch {
                let _ = inner
                    .tts
                    .retire_if(Some(&incarnation), || !server_worker_in_use(&inner));
            }
        },
    );
    Ok(false)
}

fn with_server_worker<T>(
    inner: &Arc<AppInner>,
    lifetime: &request_lifetime::RequestLifetime,
    operation: impl FnOnce(&Arc<WorkerSupervisor>) -> Result<T>,
) -> Result<T> {
    let _guard = acquire_server_job_gate(inner, lifetime)?;
    *inner
        .active_server_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(lifetime.request_id().to_owned());
    lifetime.set_stage(if inner.tts.incarnation().is_none() {
        reader_core::request_budget::RequestStage::LoadingModel
    } else {
        reader_core::request_budget::RequestStage::Inference
    });
    lifetime.check()?;
    let result = operation(&inner.tts);
    *inner
        .active_server_job
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let _ = retire_server_worker_if_idle_locked(inner);
    result
}

fn acquire_server_job_gate<'a>(
    inner: &'a Arc<AppInner>,
    lifetime: &request_lifetime::RequestLifetime,
) -> Result<std::sync::MutexGuard<'a, ()>> {
    lifetime.check()?;
    lifetime.set_stage(reader_core::request_budget::RequestStage::Queued);
    loop {
        lifetime.check()?;
        match inner.server_job_gate.try_lock() {
            Ok(guard) => {
                lifetime.check()?;
                return Ok(guard);
            }
            Err(std::sync::TryLockError::Poisoned(error)) => {
                lifetime.check()?;
                return Ok(error.into_inner());
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn requested_server_model(
    value: &serde_json::Value,
    default: &str,
    allowed: &[&str],
) -> Result<String> {
    let Some(model) = value.get("model") else {
        return Ok(default.to_owned());
    };
    let model = model.as_str().context("model must be a string")?;
    anyhow::ensure!(
        allowed.contains(&model),
        "unsupported model '{model}'; available model: {}",
        allowed.join(", ")
    );
    Ok(model.to_owned())
}

fn acquire_media_bridge<'a>(
    bridge: &'a Mutex<Option<MediaBridge>>,
    lifetime: &request_lifetime::RequestLifetime,
) -> Result<std::sync::MutexGuard<'a, Option<MediaBridge>>> {
    use std::sync::TryLockError;
    lifetime.set_stage(reader_core::request_budget::RequestStage::Queued);
    loop {
        lifetime.check()?;
        match bridge.try_lock() {
            Ok(guard) => {
                lifetime.check()?;
                return Ok(guard);
            }
            Err(TryLockError::Poisoned(error)) => {
                lifetime.check()?;
                return Ok(error.into_inner());
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

impl MediaBridge {
    fn transcribe(
        &self,
        value: serde_json::Value,
        lifetime: &request_lifetime::RequestLifetime,
    ) -> Result<serde_json::Value> {
        let (reply, response) = mpsc::channel();
        self.requests
            .send(MediaBridgeRequest { value, reply })
            .context("media bridge stopped accepting requests")?;
        loop {
            lifetime.check()?;
            let wait = lifetime
                .remaining()
                .context("media bridge request has no remaining deadline")?;
            match response.recv_timeout(wait) {
                Ok(Ok(value)) => {
                    lifetime.check()?;
                    return Ok(value);
                }
                Ok(Err(error)) => anyhow::bail!("{error}"),
                Err(RecvTimeoutError::Timeout) => lifetime.check()?,
                Err(RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("media bridge stopped before returning a response")
                }
            }
        }
    }
}

fn handle_server_request(app: &App, token: &str, mut request: tiny_http::Request) -> Result<()> {
    use base64::Engine;
    use std::io::Read;
    let auth = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| String::from_utf8_lossy(h.value.as_bytes()).to_string())
        .unwrap_or_default();
    if auth.strip_prefix("Bearer ") != Some(token) {
        let _=request.respond(server_json(401,&serde_json::json!({"error":{"message":"invalid bearer token","type":"authentication_error"}})));
        return Ok(());
    }
    let client_session = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("X-Simple-TTS-Session"))
        .map(|h| String::from_utf8_lossy(h.value.as_bytes()).to_string())
        .unwrap_or_default();
    let path = request.url().split('?').next().unwrap_or("").to_owned();
    if !matches!(
        request.method(),
        &tiny_http::Method::Post | &tiny_http::Method::Get | &tiny_http::Method::Delete
    ) {
        let _=request.respond(server_json(405,&serde_json::json!({"error":{"message":"method is not supported","type":"invalid_request_error"}})));
        return Ok(());
    }
    let config = app.inner.store.load()?;
    privacy::set_content_logging(config.privacy.log_content);
    let request_id = Uuid::new_v4().to_string();
    let lifetime = request_lifetime::RequestLifetime::new(
        request_id,
        path == "/v1/audio/transcriptions" || app.inner.tts.incarnation().is_none(),
        config.request_budgets,
    );
    let mut finish_guard = RequestFinishGuard {
        lifetime: lifetime.clone(),
        transferred: false,
    };
    let limit = if path == "/v1/chat/completions" || path == "/v1/responses" {
        16 * 1024 * 1024 + 1024 * 1024
    } else if path == "/v1/audio/transcriptions" {
        26 * 1024 * 1024
    } else {
        1_100_000
    };
    lifetime.set_stage(reader_core::request_budget::RequestStage::Queued);
    let mut body = Vec::new();
    lifetime.check()?;
    request
        .as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut body)?;
    if body.len() > limit {
        let _=request.respond(server_json(413,&serde_json::json!({"error":{"message":"request exceeds configured limit","type":"invalid_request_error"}})));
        return Ok(());
    }
    lifetime.check()?;
    let unload_epoch = app.inner.server_unload_epoch.load(Ordering::Acquire);
    let result: Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> = (|| match path.as_str() {
        "/v1/audio/voices" if *request.method() == tiny_http::Method::Get => Ok(server_json(
            200,
            &serde_json::json!({"object":"list","data":[
                {"id":"af_heart","model":"kokoro-v1"},
                {"id":"af_bella","model":"kokoro-v1"},
                {"id":"af_nicole","model":"kokoro-v1"},
                {"id":"af_sarah","model":"kokoro-v1"},
                {"id":"am_adam","model":"kokoro-v1"},
                {"id":"am_michael","model":"kokoro-v1"},
                {"id":"bf_emma","model":"kokoro-v1"},
                {"id":"sid:0","model":"supertonic3"}, {"id":"sid:1","model":"supertonic3"},
                {"id":"sid:2","model":"supertonic3"}, {"id":"sid:3","model":"supertonic3"},
                {"id":"sid:4","model":"supertonic3"}, {"id":"sid:5","model":"supertonic3"},
                {"id":"sid:6","model":"supertonic3"}, {"id":"sid:7","model":"supertonic3"},
                {"id":"sid:8","model":"supertonic3"}, {"id":"sid:9","model":"supertonic3"}
            ]}),
        )),
        "/v1/models" => Ok(server_json(200, &{
            let mut models = server_tts_models();
            models.insert(0, serde_json::json!({"id":"configured-stt","object":"model","owned_by":"simple-stt","capabilities":["stt"]}));
            models.push(serde_json::json!({"id":"simple-tts-ocr","object":"model","owned_by":"simple-tts","capabilities":["ocr"]}));
            serde_json::json!({"object":"list","data":models})
        })),
        "/v1/audio/transcriptions" if *request.method() == tiny_http::Method::Post => {
            let content_type = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Content-Type"))
                .map(|header| String::from_utf8_lossy(header.value.as_bytes()).into_owned())
                .context("multipart Content-Type is required")?;
            let (audio, language, extension) = parse_transcription_multipart(&content_type, &body)?;
            let audio_dir = reader_core::config::data_dir().join("requests");
            std::fs::create_dir_all(&audio_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&audio_dir, std::fs::Permissions::from_mode(0o700))?;
            }
            let path = audio_dir.join(format!("transcription-{}.{}", Uuid::new_v4(), extension));
            let _audio_cleanup = privacy::TemporaryFile::create(path.clone(), &audio)?;
            let write_result = (|| -> Result<String> {
                let mut bridge = acquire_media_bridge(&app.inner.media_bridge, &lifetime)?;
                let running = bridge
                    .as_mut()
                    .map(|process| process.child.try_wait().map(|status| status.is_none()))
                    .transpose()?
                    .unwrap_or(false);
                if !running {
                    *bridge = Some(start_media_bridge()?);
                }
                let process = bridge.as_mut().expect("bridge was just started");
                lifetime.set_stage(if running {
                    reader_core::request_budget::RequestStage::Inference
                } else {
                    reader_core::request_budget::RequestStage::LoadingModel
                });
                let request = serde_json::json!({
                    "path": path,
                    "language": language.as_deref(),
                });
                let value = match process.transcribe(request, &lifetime) {
                    Ok(value) => value,
                    Err(error) => {
                        bridge.take();
                        return Err(error);
                    }
                };
                anyhow::ensure!(
                    value.get("error").is_none(),
                    "{}",
                    value["error"].as_str().unwrap_or("transcription failed")
                );
                value
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .context("transcription bridge response has no transcript text")
            })();
            let _ = std::fs::remove_file(&path);
            let transcript = write_result?;
            Ok(server_json(200, &serde_json::json!({"text": transcript})))
        }
        "/app/v1/capabilities" => Ok(server_json(
            200,
            &serde_json::json!({"protocol_version":1,"server_id":format!("simple-tts-{}",std::process::id()),"warmup_leases":true,"jobs":true,"job_events":true,"cancel_jobs":true,"tts":[{"id":"kokoro-v1","voices":["af_heart","af_bella","af_nicole","af_sarah","am_adam","am_michael","bf_emma"],"languages":["en-us","en-gb"],"formats":["pcm","wav"],"pcm":{"encoding":"s16le","sample_rate_hz":24000,"channels":1},"timing":"sentence"}],"ocr":[{"id":"paddle-ar-en","languages":["eng"],"word_boxes":false}]}),
        )),
        "/app/v1/warmups" if *request.method() == tiny_http::Method::Post => {
            anyhow::ensure!(
                uuid::Uuid::parse_str(&client_session).is_ok(),
                "X-Simple-TTS-Session must be a UUID"
            );
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            let models = value
                .get("models")
                .and_then(|v| v.as_array())
                .context("warmup requires models")?;
            anyhow::ensure!(
                models
                    .iter()
                    .all(|v| matches!(v.as_str(), Some("kokoro-v1" | "paddle-ar-en"))),
                "warmup requested an uninstalled model alias"
            );
            let lease_ms = value
                .get("lease_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(15_000)
                .clamp(1_000, 300_000);
            let lease = Uuid::new_v4().to_string();
            let mut retained = true;
            if models.iter().any(|v| v.as_str() == Some("kokoro-v1")) {
                let _queue = app
                    .inner
                    .server_job_gate
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *app.inner
                    .active_server_job
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(lease.clone());
                let language = if config.speech.selection.mode
                    == reader_core::routing::SelectionMode::FollowKeyboard
                {
                    query_keyboard_language().ok()
                } else {
                    None
                };
                let prepared =
                    reading_hint::prewarm(&config, language.as_deref()).and_then(|route| {
                        app.inner.tts.prepare_with_cancel(&route, None, None, || {
                            app.inner.server_unload_epoch.load(Ordering::Acquire) == unload_epoch
                        })
                    });
                *app.inner
                    .active_server_job
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                if let Err(error) = prepared {
                    let _ = retire_server_worker_if_idle_locked(&app.inner);
                    return Err(error);
                }
                app.inner.warmups.lock().unwrap().insert(
                    lease.clone(),
                    (
                        client_session.clone(),
                        Instant::now() + Duration::from_millis(lease_ms),
                    ),
                );
                let _ = retire_server_worker_if_idle_locked(&app.inner)?;
                retained = app.inner.warmups.lock().unwrap().contains_key(&lease);
            } else {
                app.inner.warmups.lock().unwrap().insert(
                    lease.clone(),
                    (
                        client_session.clone(),
                        Instant::now() + Duration::from_millis(lease_ms),
                    ),
                );
            }
            let inner = Arc::downgrade(&app.inner);
            let id = lease.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(lease_ms));
                if let Some(inner) = inner.upgrade() {
                    let _queue = inner
                        .server_job_gate
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut leases = inner.warmups.lock().unwrap();
                    leases.remove(&id);
                    leases.retain(|_, (_, expires)| *expires > Instant::now());
                    if leases.is_empty() {
                        drop(leases);
                        let _ = retire_server_worker_if_idle_locked(&inner);
                    }
                }
            });
            Ok(server_json(
                200,
                &serde_json::json!({"lease_id":lease,"lease_ms":if retained { lease_ms } else { 0 },"status":if retained { "prepared" } else { "released" }}),
            ))
        }
        _ if path.starts_with("/app/v1/warmups/")
            && *request.method() == tiny_http::Method::Delete =>
        {
            let _queue = app
                .inner
                .server_job_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let id = path.trim_start_matches("/app/v1/warmups/");
            let mut leases = app.inner.warmups.lock().unwrap();
            if leases
                .get(id)
                .is_some_and(|(owner, _)| owner == &client_session && !client_session.is_empty())
            {
                leases.remove(id);
            } else {
                return Ok(server_json(
                    404,
                    &serde_json::json!({"error":{"message":"warmup lease not found","type":"invalid_request_error"}}),
                ));
            }
            if leases.is_empty() {
                drop(leases);
                let _ = retire_server_worker_if_idle_locked(&app.inner);
            }
            Ok(server_json(200, &serde_json::json!({"released":true})))
        }
        "/app/v1/jobs" if *request.method() == tiny_http::Method::Post => {
            anyhow::ensure!(
                uuid::Uuid::parse_str(&client_session).is_ok(),
                "X-Simple-TTS-Session must be a UUID"
            );
            let input: serde_json::Value = serde_json::from_slice(&body)?;
            let text = input
                .get("input")
                .and_then(|v| v.as_str())
                .context("request requires input text")?;
            anyhow::ensure!(
                text.len() <= config.server.max_text_bytes,
                "speech input exceeds configured text limit"
            );
            let voice = input
                .get("voice")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let speed = input
                .get("speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(config.speech.speed as f64) as f32;
            anyhow::ensure!(
                speed.is_finite() && (0.5..=2.0).contains(&speed),
                "speed must be between 0.5 and 2.0"
            );
            let format = input
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("pcm")
                .to_owned();
            anyhow::ensure!(
                matches!(format.as_str(), "pcm" | "wav"),
                "format must be pcm or wav"
            );
            let config_for_thread = if input.get("model").is_some() {
                server_tts_route(&config, &input, text)?
            } else {
                routed_media_config(&config, text)?
            };
            let mut jobs = app.inner.server_jobs.lock().unwrap();
            jobs.retain(|_, job| {
                job.created.elapsed() < Duration::from_secs(600)
                    || matches!(job.state.lock().unwrap().as_str(), "queued" | "running")
            });
            anyhow::ensure!(
                jobs.len() < MAX_SERVER_JOBS,
                "server job retention limit reached"
            );
            anyhow::ensure!(
                jobs.values()
                    .filter(|job| matches!(
                        job.state.lock().unwrap().as_str(),
                        "queued" | "running"
                    ))
                    .count()
                    < MAX_ACTIVE_SERVER_JOBS,
                "server job queue is full"
            );
            let id = lifetime.request_id().to_owned();
            let generation = app
                .inner
                .server_job_generation
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            let job = Arc::new(ServerJob {
                id: id.clone(),
                generation,
                owner: client_session.clone(),
                state: Mutex::new("queued".into()),
                events: Mutex::new(Vec::new()),
                manifest: Mutex::new(None),
                audio: Mutex::new(None),
                created: Instant::now(),
                lifetime: lifetime.clone(),
            });
            push_job_event(&job, "queued", serde_json::json!({}));
            jobs.insert(id.clone(), Arc::clone(&job));
            drop(jobs);
            let job_for_thread = Arc::clone(&job);
            let app_for_thread = app.clone();
            let text = text.to_owned();
            std::thread::spawn(move || {
                let _queue_guard = match acquire_server_job_gate(
                    &app_for_thread.inner,
                    &job_for_thread.lifetime,
                ) {
                    Ok(guard) => guard,
                    Err(error) => {
                        *job_for_thread.state.lock().unwrap() = "failed".into();
                        push_job_event(
                            &job_for_thread,
                            "failed",
                            serde_json::json!({"message":error.to_string()}),
                        );
                        job_for_thread.lifetime.finish();
                        return;
                    }
                };
                let started = {
                    let mut state = job_for_thread.state.lock().unwrap();
                    if *state == "queued"
                        && app_for_thread
                            .inner
                            .server_unload_epoch
                            .load(Ordering::Acquire)
                            == unload_epoch
                    {
                        *state = "running".into();
                        true
                    } else {
                        if *state == "queued" {
                            *state = "cancelled".into();
                            push_job_event(&job_for_thread, "cancelled", serde_json::json!({}));
                        }
                        false
                    }
                };
                if !started {
                    job_for_thread.lifetime.finish();
                    let _ = retire_server_worker_if_idle_locked(&app_for_thread.inner);
                    return;
                }
                *app_for_thread.inner.active_server_job.lock().unwrap() =
                    Some(job_for_thread.id.clone());
                push_job_event(&job_for_thread, "started", serde_json::json!({}));
                let generated =
                    (|| -> Result<(Vec<i16>, u32, Vec<serde_json::Value>, AppConfig)> {
                        let mut manifest = Vec::new();
                        let is_current = || {
                            *job_for_thread.state.lock().unwrap() == "running"
                                && job_for_thread.lifetime.check().is_ok()
                        };
                        let (samples, last_route) = collect_routed_media(
                            &app_for_thread.inner.tts,
                            config_for_thread,
                            &text,
                            MediaOptions {
                                voice: voice.as_deref(),
                                speed,
                                max_samples: MAX_SERVER_JOB_AUDIO / 2,
                                request_id: Some(&job_for_thread.id),
                                lifetime: Some(&job_for_thread.lifetime),
                                current: &is_current,
                            },
                            |unit, start, count| {
                                job_for_thread.lifetime.set_stage(
                                    reader_core::request_budget::RequestStage::Streaming,
                                );
                                job_for_thread.lifetime.progress();
                                let index = manifest.len();
                                manifest.push(serde_json::json!({"utterance_index":index,"sentence_index":unit.sentence_index,"unit_index":unit.unit_index,"text":unit.text,"sample_offset":start,"sample_count":count,"duration_seconds":count as f64/media_routing::OUTPUT_RATE as f64}));
                                push_job_event(
                                    &job_for_thread,
                                    "utterance_ready",
                                    serde_json::json!({"utterance_index":index,"sentence_index":unit.sentence_index,"unit_index":unit.unit_index,"sample_offset":start,"sample_count":count}),
                                );
                            },
                        )?;
                        Ok((samples, media_routing::OUTPUT_RATE, manifest, last_route))
                    })();
                match generated {
                    Ok((samples, rate, utterances, last_route))
                        if *job_for_thread.state.lock().unwrap() == "running" =>
                    {
                        let manifest = serde_json::json!({"job_id":job_for_thread.id,"generation":job_for_thread.generation,"encoding":"s16le","sample_rate_hz":rate,"channels":1,"utterances":utterances});
                        let audio = if format == "wav" {
                            let mut cursor = std::io::Cursor::new(Vec::new());
                            let mut writer = hound::WavWriter::new(
                                &mut cursor,
                                hound::WavSpec {
                                    channels: 1,
                                    sample_rate: rate,
                                    bits_per_sample: 16,
                                    sample_format: hound::SampleFormat::Int,
                                },
                            )
                            .expect("valid PCM WAV spec");
                            for sample in samples {
                                if let Err(error) = writer.write_sample(sample) {
                                    *job_for_thread.state.lock().unwrap() = "failed".into();
                                    push_job_event(
                                        &job_for_thread,
                                        "failed",
                                        serde_json::json!({"message":error.to_string()}),
                                    );
                                    break;
                                }
                            }
                            let _ = writer.finalize();
                            cursor.into_inner()
                        } else {
                            let mut bytes = Vec::with_capacity(samples.len() * 2);
                            for sample in samples {
                                bytes.extend_from_slice(&sample.to_le_bytes())
                            }
                            bytes
                        };
                        let retained = app_for_thread
                            .inner
                            .server_jobs
                            .lock()
                            .unwrap()
                            .values()
                            .map(|other| {
                                other
                                    .audio
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .map_or(0, |(data, _)| data.len())
                            })
                            .sum::<usize>();
                        if audio.len() > MAX_SERVER_JOB_AUDIO
                            || retained.saturating_add(audio.len()) > MAX_SERVER_AUDIO_STORAGE
                        {
                            *job_for_thread.state.lock().unwrap() = "failed".into();
                            push_job_event(
                                &job_for_thread,
                                "failed",
                                serde_json::json!({"message":"server audio retention limit reached"}),
                            );
                        } else {
                            *job_for_thread.manifest.lock().unwrap() = Some(manifest);
                            *job_for_thread.audio.lock().unwrap() = Some((
                                audio,
                                if format == "wav" {
                                    "audio/wav".into()
                                } else {
                                    format!("audio/pcm;rate={rate};channels=1")
                                },
                            ));
                            if *job_for_thread.state.lock().unwrap() == "running" {
                                *job_for_thread.state.lock().unwrap() = "completed".into();
                                if let Err(error) = reading_hint::record(&last_route) {
                                    tracing::debug!(%error, "could not save last voice route");
                                }
                                push_job_event(
                                    &job_for_thread,
                                    "completed",
                                    serde_json::json!({"manifest_url":format!("/app/v1/jobs/{}/manifest",job_for_thread.id),"audio_url":format!("/app/v1/jobs/{}/audio",job_for_thread.id)}),
                                );
                            }
                        }
                    }
                    Err(error) if *job_for_thread.state.lock().unwrap() == "running" => {
                        *job_for_thread.state.lock().unwrap() = "failed".into();
                        push_job_event(
                            &job_for_thread,
                            "failed",
                            serde_json::json!({"message":error.to_string()}),
                        );
                    }
                    _ => {}
                }
                *app_for_thread.inner.active_server_job.lock().unwrap() = None;
                job_for_thread.lifetime.finish();
                let _ = retire_server_worker_if_idle_locked(&app_for_thread.inner);
            });
            finish_guard.transferred = true;
            Ok(server_json(
                202,
                &serde_json::json!({"id":id,"generation":generation,"status":"queued","events_url":format!("/app/v1/jobs/{id}/events"),"manifest_url":format!("/app/v1/jobs/{id}/manifest"),"audio_url":format!("/app/v1/jobs/{id}/audio")}),
            ))
        }
        _ if path.starts_with("/app/v1/jobs/") => {
            let tail = path.trim_start_matches("/app/v1/jobs/");
            let mut parts = tail.split('/');
            let id = parts.next().unwrap_or("");
            let sub = parts.next();
            let job = app
                .inner
                .server_jobs
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .filter(|job| job.owner == client_session && !client_session.is_empty());
            let Some(job) = job else {
                return Ok(server_json(
                    404,
                    &serde_json::json!({"error":{"message":"job not found","type":"invalid_request_error"}}),
                ));
            };
            match (sub, request.method()) {
                (None, &tiny_http::Method::Get) => Ok(server_json(
                    200,
                    &serde_json::json!({"id":id,"status":*job.state.lock().unwrap()}),
                )),
                (Some("events"), &tiny_http::Method::Get) => {
                    let after = request
                        .url()
                        .split_once("after=")
                        .and_then(|(_, v)| v.split('&').next())
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    let events = job.events.lock().unwrap();
                    Ok(server_json(
                        200,
                        &serde_json::json!({"events":events.iter().filter(|event|event.get("sequence").and_then(|v|v.as_u64()).unwrap_or(0)>=after as u64).cloned().collect::<Vec<_>>()}),
                    ))
                }
                (Some("manifest"), &tiny_http::Method::Get) => {
                    let manifest = job.manifest.lock().unwrap();
                    Ok(server_json(
                        200,
                        manifest.as_ref().context("job manifest is not ready")?,
                    ))
                }
                (Some("audio"), &tiny_http::Method::Get) => {
                    let audio = job.audio.lock().unwrap();
                    let (data, content_type) = audio.as_ref().context("job audio is not ready")?;
                    Ok(tiny_http::Response::from_data(data.clone())
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", content_type.as_str())
                                .unwrap(),
                        )
                        .with_header(
                            tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap(),
                        ))
                }
                (None, &tiny_http::Method::Delete) => {
                    let cancelled = {
                        let mut state = job.state.lock().unwrap();
                        if matches!(state.as_str(), "queued" | "running") {
                            *state = "cancelled".into();
                            true
                        } else {
                            false
                        }
                    };
                    if cancelled {
                        job.lifetime.cancel();
                        // The shared worker is owned by whichever utterance is
                        // currently running. The job's state fences its next
                        // chunk; cancellation must not kill another owner's
                        // in-flight desktop utterance.
                        push_job_event(&job, "cancelled", serde_json::json!({}));
                    }
                    job.audio.lock().unwrap().take();
                    job.manifest.lock().unwrap().take();
                    job.events.lock().unwrap().clear();
                    app.inner.server_jobs.lock().unwrap().remove(id);
                    Ok(server_json(
                        200,
                        &serde_json::json!({"id":id,"status":*job.state.lock().unwrap()}),
                    ))
                }
                _ => Ok(server_json(
                    404,
                    &serde_json::json!({"error":{"message":"job endpoint not found","type":"invalid_request_error"}}),
                )),
            }
        }
        "/v1/audio/speech" => {
            let input: serde_json::Value = serde_json::from_slice(&body)?;
            let text = input
                .get("input")
                .and_then(|v| v.as_str())
                .context("request requires input text")?;
            anyhow::ensure!(
                text.len() <= config.server.max_text_bytes,
                "speech input exceeds configured text limit"
            );
            let voice = input.get("voice").and_then(|v| v.as_str());
            let speed = input
                .get("speed")
                .and_then(|v| v.as_f64())
                .unwrap_or(config.speech.speed as f64) as f32;
            anyhow::ensure!(
                speed.is_finite() && (0.5..=2.0).contains(&speed),
                "speed must be between 0.5 and 2.0"
            );
            let format = input
                .get("response_format")
                .and_then(|v| v.as_str())
                .unwrap_or("wav");
            anyhow::ensure!(
                matches!(format, "wav" | "pcm"),
                "response_format must be wav or pcm"
            );
            let route = server_tts_route(&config, &input, text)?;
            let (samples, last_route) = with_server_worker(&app.inner, &lifetime, |worker| {
                let is_current = || {
                    !app.inner.shutdown.load(Ordering::Acquire)
                        && app.inner.server_unload_epoch.load(Ordering::Acquire) == unload_epoch
                        && lifetime.check().is_ok()
                };
                collect_routed_media(
                    worker,
                    route,
                    text,
                    MediaOptions {
                        voice,
                        speed,
                        max_samples: MAX_SERVER_JOB_AUDIO / 2,
                        request_id: Some(lifetime.request_id()),
                        lifetime: Some(&lifetime),
                        current: &is_current,
                    },
                    |_, _, _| {},
                )
            })?;
            let rate = media_routing::OUTPUT_RATE;
            if format == "pcm" {
                let mut data = Vec::with_capacity(samples.len() * 2);
                for sample in samples {
                    data.extend_from_slice(&sample.to_le_bytes())
                }
                if let Err(error) = reading_hint::record(&last_route) {
                    tracing::debug!(%error, "could not save last voice route");
                }
                Ok(tiny_http::Response::from_data(data)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            "Content-Type",
                            format!("audio/pcm;rate={rate};channels=1"),
                        )
                        .unwrap(),
                    )
                    .with_header(
                        tiny_http::Header::from_bytes("X-Audio-Encoding", "s16le").unwrap(),
                    ))
            } else {
                let mut cursor = std::io::Cursor::new(Vec::new());
                {
                    let mut writer = hound::WavWriter::new(
                        &mut cursor,
                        hound::WavSpec {
                            channels: 1,
                            sample_rate: rate,
                            bits_per_sample: 16,
                            sample_format: hound::SampleFormat::Int,
                        },
                    )?;
                    for sample in samples {
                        writer.write_sample(sample)?
                    }
                    writer.finalize()?;
                }
                if let Err(error) = reading_hint::record(&last_route) {
                    tracing::debug!(%error, "could not save last voice route");
                }
                Ok(
                    tiny_http::Response::from_data(cursor.into_inner()).with_header(
                        tiny_http::Header::from_bytes("Content-Type", "audio/wav").unwrap(),
                    ),
                )
            }
        }
        "/v1/chat/completions" => {
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            let model = requested_server_model(
                &value,
                "simple-tts-ocr",
                &["simple-tts-ocr", "paddle-ar-en"],
            )?;
            let messages = value
                .get("messages")
                .and_then(|v| v.as_array())
                .context("request requires messages")?;
            let mut image_url = None;
            let mut prompt = String::new();
            for message in messages {
                if let Some(content) = message.get("content") {
                    if let Some(text) = content.as_str() {
                        prompt.push_str(text)
                    } else if let Some(parts) = content.as_array() {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                prompt.push_str(text)
                            }
                            if let Some(url) = part
                                .pointer("/image_url/url")
                                .and_then(|v| v.as_str())
                                .or_else(|| part.get("image_url").and_then(|v| v.as_str()))
                            {
                                image_url = Some(url.to_owned())
                            }
                        }
                    }
                }
            }
            let url = image_url.context("chat completion requires an image_url")?;
            let encoded = url
                .split_once(",")
                .map(|v| v.1)
                .context("image_url must be a base64 data URL")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            let text = ocr_server_image(app, &bytes, unload_epoch, &lifetime)?;
            Ok(server_json(
                200,
                &serde_json::json!({"id":format!("chatcmpl-{}",Uuid::new_v4()),"object":"chat.completion","created":0,"model":model,"choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}]}),
            ))
        }
        "/v1/responses" => {
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            let model = requested_server_model(
                &value,
                "simple-tts-ocr",
                &["simple-tts-ocr", "paddle-ar-en"],
            )?;
            let input = value
                .get("input")
                .and_then(|v| v.as_array())
                .context("OCR Responses input must be an array")?;
            let mut image_url = None;
            for message in input {
                if let Some(parts) = message.get("content").and_then(|v| v.as_array()) {
                    for part in parts {
                        if let Some(url) = part
                            .get("image_url")
                            .and_then(|v| v.as_str())
                            .or_else(|| part.pointer("/image_url/url").and_then(|v| v.as_str()))
                        {
                            image_url = Some(url.to_owned())
                        }
                    }
                }
            }
            let url = image_url.context("Responses input requires an input_image")?;
            let encoded = url
                .split_once(',')
                .map(|v| v.1)
                .context("input_image must be a base64 data URL")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            let text = ocr_server_image(app, &bytes, unload_epoch, &lifetime)?;
            Ok(server_json(
                200,
                &serde_json::json!({"id":format!("resp_{}",Uuid::new_v4()),"object":"response","status":"completed","model":model,"output":[{"id":format!("msg_{}",Uuid::new_v4()),"type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]}]}),
            ))
        }
        _ => bail!("no OpenAI-compatible route exists at {path}"),
    })();
    let response=result.unwrap_or_else(|error|{tracing::warn!(%error,"API request rejected");server_json(400,&serde_json::json!({"error":{"message":error.to_string(),"type":"invalid_request_error"}}))});
    let _ = request.respond(response);
    Ok(())
}
fn parse_transcription_multipart(
    content_type: &str,
    body: &[u8],
) -> Result<(Vec<u8>, Option<String>, String)> {
    anyhow::ensure!(
        content_type
            .split(';')
            .next()
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("multipart/form-data")),
        "Content-Type must be multipart/form-data"
    );
    let boundary = content_type
        .split(';')
        .skip(1)
        .filter_map(|part| part.trim().strip_prefix("boundary="))
        .map(|value| value.trim().trim_matches('"'))
        .next()
        .context("multipart boundary is missing")?;
    anyhow::ensure!(
        !boundary.is_empty()
            && boundary.len() <= 70
            && boundary.bytes().all(|byte| byte.is_ascii_graphic()),
        "multipart boundary is invalid"
    );
    let delimiter = [b"--".as_slice(), boundary.as_bytes()].concat();
    let separator = [b"\r\n".as_slice(), delimiter.as_slice()].concat();
    let mut cursor = 0usize;
    let mut file = None;
    let mut extension = "wav".to_owned();
    let mut language = None;
    while cursor < body.len() {
        anyhow::ensure!(
            body[cursor..].starts_with(&delimiter),
            "multipart boundary is malformed"
        );
        cursor += delimiter.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        anyhow::ensure!(
            body.get(cursor..cursor + 2) == Some(b"\r\n"),
            "multipart boundary is malformed"
        );
        cursor += 2;
        let headers_end = find_bytes(body, b"\r\n\r\n", cursor)
            .context("multipart part headers are incomplete")?;
        let headers = std::str::from_utf8(&body[cursor..headers_end])
            .context("multipart headers are not UTF-8")?;
        cursor = headers_end + 4;
        let part_end = find_bytes(body, &separator, cursor)
            .context("multipart closing boundary is missing")?;
        let part = &body[cursor..part_end];
        let disposition = headers
            .lines()
            .find(|line| {
                line.split_once(':')
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("Content-Disposition"))
            })
            .and_then(|line| line.split_once(':').map(|(_, value)| value.trim()))
            .context("multipart part has no Content-Disposition")?;
        anyhow::ensure!(
            disposition.to_ascii_lowercase().starts_with("form-data;"),
            "multipart part must use form-data"
        );
        let name = disposition_parameter(disposition, "name")
            .context("multipart part has no field name")?;
        match name.as_str() {
            "file" => {
                anyhow::ensure!(
                    file.is_none(),
                    "multipart request has more than one file field"
                );
                anyhow::ensure!(!part.is_empty(), "audio file is empty");
                if let Some(filename) = disposition_parameter(disposition, "filename") {
                    if let Some(candidate) = Path::new(&filename)
                        .extension()
                        .and_then(|value| value.to_str())
                        .map(str::to_ascii_lowercase)
                        .filter(|value| {
                            matches!(
                                value.as_str(),
                                "wav"
                                    | "mp3"
                                    | "m4a"
                                    | "flac"
                                    | "ogg"
                                    | "webm"
                                    | "mp4"
                                    | "mpeg"
                                    | "mpga"
                                    | "opus"
                            )
                        })
                    {
                        extension = candidate;
                    }
                }
                file = Some(part.to_vec());
            }
            "language" => {
                let value = std::str::from_utf8(part)
                    .context("language field is not UTF-8")?
                    .trim();
                anyhow::ensure!(
                    value.len() <= 128 && !value.chars().any(char::is_control),
                    "language field is invalid"
                );
                language = (!value.is_empty()).then(|| value.to_owned());
            }
            "model" => {} // The selected speech configuration is authoritative.
            _ => {}
        }
        cursor = part_end + 2;
    }
    Ok((
        file.context("multipart request requires a file field")?,
        language,
        extension,
    ))
}

fn find_bytes(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

fn disposition_parameter(disposition: &str, key: &str) -> Option<String> {
    disposition
        .split(';')
        .skip(1)
        .filter_map(|parameter| parameter.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case(key))
        .map(|(_, value)| value.trim().trim_matches('"').to_owned())
}

fn server_json(
    status: u16,
    value: &serde_json::Value,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_data(value.to_string().into_bytes())
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
}

fn spawn_reaped(mut command: Command) -> Result<()> {
    let mut child = command.spawn().context("starting desktop helper")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn text_fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    hasher.finish()
}

fn text_preview(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(54).collect::<String>()
        + if compact.chars().count() > 54 {
            "…"
        } else {
            ""
        }
}
fn write_device_report(device: &str, ready: bool) {
    let path = data_dir().join("device-report.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    };
    let value = serde_json::json!({"device":device,"ready":ready,"updated_at":format!("{:?}",Instant::now())});
    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = std::fs::File::create(&tmp) {
        let _ = file.write_all(value.to_string().as_bytes());
        let _ = std::fs::rename(tmp, path);
    }
}

#[cfg(unix)]
fn serve_daemon(app: App, no_desktop: bool) -> Result<()> {
    let _processing = privacy::ProcessingLease::acquire(&data_dir())?;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    app.inner
        .desktop_enabled
        .store(!no_desktop, Ordering::Relaxed);
    let path = data_dir().join("simple-tts.sock");
    std::fs::create_dir_all(data_dir())?;
    std::fs::set_permissions(data_dir(), std::fs::Permissions::from_mode(0o700))?;
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("Simple TTS controller is already running")
        };
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path).context("binding local controller socket")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    eprintln!("Simple TTS controller is ready");
    app.inner
        .desktop_enabled
        .store(!no_desktop, Ordering::Relaxed);
    if !no_desktop {
        start_desktop_integrations(app.clone());
    }
    if app.inner.store.load()?.server.enabled {
        if let Err(error) = app.start_server() {
            tracing::error!(%error,"configured headless server could not start");
        }
    }
    let signal_app = app.clone();
    ctrlc::set_handler(move || request_daemon_shutdown(&signal_app))?;
    let active = Arc::new(AtomicUsize::new(0));
    while !app.inner.shutdown.load(Ordering::Relaxed) {
        let (mut stream, _) = listener.accept()?;
        if active.load(Ordering::Relaxed) >= 16 {
            let _ = write_json_frame(
                &mut stream,
                &LocalReply::error("too many connected clients"),
            );
            continue;
        }
        active.fetch_add(1, Ordering::Relaxed);
        let app = app.clone();
        let active = active.clone();
        let wake_path = path.clone();
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(90)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
            let request = read_json_frame::<LocalRequest>(&mut stream);
            let should_shutdown = request
                .as_ref()
                .is_ok_and(|value| matches!(value, LocalRequest::Shutdown));
            let result = request.and_then(|request| app.handle(request));
            let reply = result.unwrap_or_else(|e| LocalReply::error(format!("{e:#}")));
            let _ = write_json_frame(&mut stream, &reply);
            if should_shutdown {
                app.inner.shutdown.store(true, Ordering::Relaxed);
                let _ = UnixStream::connect(wake_path);
            }
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
    app.handle(LocalRequest::Shutdown)?;
    let _ = std::fs::remove_file(path);
    Ok(())
}
fn request_daemon_shutdown(app: &App) {
    app.inner.shutdown.store(true, Ordering::Release);
    #[cfg(unix)]
    wake_unix_socket();
    #[cfg(windows)]
    wake_pipe();
}
#[cfg(unix)]
fn wake_unix_socket() {
    if let Ok(stream) = std::os::unix::net::UnixStream::connect(data_dir().join("simple-tts.sock"))
    {
        drop(stream);
    }
}
#[cfg(windows)]
fn pipe_name() -> Vec<u16> {
    format!(
        r"\\.\pipe\simple-stt-reader-{}",
        std::env::var("USERNAME").unwrap_or_else(|_| "current-user".into())
    )
    .encode_utf16()
    .chain(std::iter::once(0))
    .collect()
}
#[cfg(windows)]
fn wake_pipe() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    let name = pipe_name();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle != INVALID_HANDLE_VALUE {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
    }
}
#[cfg(windows)]
fn serve_daemon(app: App, no_desktop: bool) -> Result<()> {
    let _processing = privacy::ProcessingLease::acquire(&data_dir())?;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    app.inner
        .desktop_enabled
        .store(!no_desktop, Ordering::Relaxed);
    if !no_desktop {
        start_desktop_integrations(app.clone());
    }
    if app.inner.store.load()?.server.enabled {
        app.start_server()?;
    }
    let signal_app = app.clone();
    ctrlc::set_handler(move || request_daemon_shutdown(&signal_app))?;
    let active = Arc::new(AtomicUsize::new(0));
    let name = pipe_name();
    while !app.inner.shutdown.load(Ordering::Relaxed) {
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                16,
                65_536,
                65_536,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            bail!("CreateNamedPipeW failed ({})", unsafe {
                windows_sys::Win32::Foundation::GetLastError()
            })
        };
        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if connected == 0
            && unsafe { windows_sys::Win32::Foundation::GetLastError() } != ERROR_PIPE_CONNECTED
        {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            continue;
        }
        let mut stream = unsafe { std::fs::File::from_raw_handle(handle as RawHandle) };
        if active.load(Ordering::Relaxed) >= 16 {
            let _ = write_json_frame(
                &mut stream,
                &LocalReply::error("too many connected clients"),
            );
            continue;
        }
        active.fetch_add(1, Ordering::Relaxed);
        let app = app.clone();
        let active = active.clone();
        std::thread::spawn(move || {
            let request = read_json_frame::<LocalRequest>(&mut stream);
            let should_shutdown = request
                .as_ref()
                .is_ok_and(|value| matches!(value, LocalRequest::Shutdown));
            let result = request.and_then(|request| app.handle(request));
            let reply = result.unwrap_or_else(|error| LocalReply::error(format!("{error:#}")));
            let _ = write_json_frame(&mut stream, &reply);
            if should_shutdown {
                app.inner.shutdown.store(true, Ordering::Relaxed);
                wake_pipe();
            }
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}
#[cfg(not(any(unix, windows)))]
fn serve_daemon(_app: App, _no_desktop: bool) -> Result<()> {
    bail!("local controller transport is unsupported on this OS")
}

fn start_desktop_integrations(app: App) {
    #[cfg(target_os = "linux")]
    {
        if let Err(error) = futures_lite::future::block_on(portal_identity::register()) {
            tracing::warn!(%error, "portal application identity is unavailable; this desktop may display Simple STT as an unidentified application");
        }
        let memory_app = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = futures_lite::future::block_on(memory_pressure_listener(memory_app))
            {
                tracing::debug!(%error, "desktop low-memory signal is unavailable");
            }
        });
        let suspend_app = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = futures_lite::future::block_on(suspend_listener(suspend_app)) {
                tracing::debug!(%error, "system suspend signal is unavailable");
            }
        });
        if std::env::var_os("SIMPLE_STT_EMBEDDED_READER").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            let portal = app.clone();
            std::thread::spawn(move || {
                let mut delay = 2u64;
                loop {
                    let result = futures_lite::future::block_on(portal_shortcuts(portal.clone()));
                    tracing::warn!(
                        ?result,
                        "global shortcut portal ended; retrying with backoff"
                    );
                    std::thread::sleep(Duration::from_secs(delay));
                    delay = (delay * 2).min(60);
                }
            });
        }
        let tray_app = app.clone();
        if std::env::var_os("SIMPLE_STT_EMBEDDED_READER").is_none() {
            std::thread::spawn(move || start_tray(tray_app));
        }
        if std::env::var_os("SIMPLE_STT_EMBEDDED_READER").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var_os("DISPLAY").is_some()
        {
            let hotkeys = app.clone();
            std::thread::spawn(move || {
                if let Err(error) = x11_hotkeys(hotkeys) {
                    tracing::error!(%error,"X11 global shortcuts stopped");
                }
            });
        }
    }
    #[cfg(windows)]
    {
        let tray_app = app.clone();
        if std::env::var_os("SIMPLE_STT_EMBEDDED_READER").is_none() {
            std::thread::spawn(move || windows_tray(tray_app));
        }
        let hotkeys = app.clone();
        if std::env::var_os("SIMPLE_STT_EMBEDDED_READER").is_none() {
            std::thread::spawn(move || {
                if let Err(error) = windows_hotkeys(hotkeys) {
                    tracing::error!(%error,"Windows global shortcuts stopped");
                }
            });
        }
    }
}

#[cfg(target_os = "linux")]
async fn memory_pressure_listener(app: App) -> Result<()> {
    use futures_lite::StreamExt;

    let monitor = ashpd::desktop::memory_monitor::MemoryMonitor::new().await?;
    let mut warnings = monitor.receive_low_memory_warning().await?;
    while let Some(level) = warnings.next().await {
        let generation = app.inner.generation.current();
        let Some(incarnation) = app.inner.tts.try_incarnation() else {
            continue;
        };
        let retired = app.inner.tts.retire_if(Some(&incarnation), || {
            app.inner.generation.accepts(generation)
                && audio_is_idle(&app.inner.audio)
                && app
                    .inner
                    .session
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_ref()
                    .is_none_or(|session| session.generation == generation)
        });
        if matches!(retired, Ok(true)) {
            write_device_report(
                &format!("Stopped · system memory pressure ({level})"),
                false,
            );
        } else if let Err(error) = retired {
            tracing::warn!(%error,"memory-pressure worker cleanup failed");
        }
    }
    bail!("desktop low-memory signal stream ended")
}

#[cfg(target_os = "linux")]
async fn suspend_listener(app: App) -> Result<()> {
    use futures_lite::StreamExt;

    let connection = zbus::Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let mut signals = proxy.receive_signal("PrepareForSleep").await?;
    while let Some(signal) = signals.next().await {
        let sleeping = signal.body().deserialize::<bool>()?;
        handle_power_transition(app.clone(), sleeping);
    }
    bail!("system suspend signal stream ended")
}

fn handle_power_transition(app: App, sleeping: bool) {
    if sleeping {
        if let Err(error) = app.action_stop() {
            tracing::warn!(%error,"could not stop reading before system suspend");
        }
        if let Err(error) = app.stop_server_workers() {
            tracing::warn!(%error,"could not stop API workers before system suspend");
        }
    } else if app
        .inner
        .store
        .load()
        .is_ok_and(|config| config.server.enabled)
    {
        if let Err(error) = app.start_server() {
            tracing::warn!(%error,"could not restart the API server after system resume");
        }
    }
}

fn dispatch_shortcut(app: App, id: String) {
    let action = match id.as_str() {
        "abort" => app.action_stop().map(|_| ()),
        "read-selection" => read_selection_hotkey(&app),
        "read-region" => read_region_hotkey(&app),
        "read-along" => app.action("read-along").map(|_| ()),
        "read-along-next" => app.action("read-along-next").map(|_| ()),
        "copy-region" => app.action("copy-region").map(|_| ()),
        "listen-clipboard" => app.action_toggle_clipboard_listening().map(|_| ()),
        _ => Ok(()),
    };
    if let Err(error) = action {
        tracing::warn!(%error,shortcut=%id,"shortcut action failed")
    }
}

fn shortcut_parts(binding: &str) -> Option<(Vec<&str>, &str)> {
    let mut parts = binding.split('+').map(str::trim);
    let mut modifiers = Vec::new();
    let key = loop {
        let part = parts.next()?;
        if parts.clone().next().is_none() {
            break part;
        }
        modifiers.push(part)
    };
    Some((modifiers, key))
}

#[cfg(target_os = "linux")]
fn xdg_trigger(binding: &str) -> Option<String> {
    let (modifiers, key) = shortcut_parts(binding)?;
    let mut trigger = String::new();
    for modifier in modifiers {
        trigger.push_str(match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "<Control>",
            "alt" => "<Alt>",
            "shift" => "<Shift>",
            "meta" | "super" | "win" => "<Super>",
            _ => return None,
        });
    }
    let key = match key.to_ascii_lowercase().as_str() {
        "escape" => "Escape".to_owned(),
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "up" => "Up".to_owned(),
        "down" => "Down".to_owned(),
        "space" => "space".to_owned(),
        "return" | "enter" => "Return".to_owned(),
        "tab" => "Tab".to_owned(),
        "backspace" => "BackSpace".to_owned(),
        value if value.len() == 1 => value.to_owned(),
        value if value.starts_with('f') && value[1..].parse::<u8>().is_ok() => {
            value.to_ascii_uppercase()
        }
        _ => return None,
    };
    trigger.push_str(&key);
    Some(trigger)
}

#[cfg(target_os = "linux")]
fn x11_hotkeys(app: App) -> Result<()> {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{ConnectionExt, GrabMode, KeyPressEvent, ModMask},
    };
    let config = app.inner.store.load()?;
    let (c, screen) = x11rb::connect(None)?;
    let root = c.setup().roots[screen].root;
    let setup = c.setup();
    let first = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(first).saturating_add(1);
    let mapping = c.get_keyboard_mapping(first, count)?.reply()?;
    let per_key = mapping.keysyms_per_keycode as usize;
    let bindings = [
        ("read-selection", config.shortcuts.read_selection),
        ("read-region", config.shortcuts.read_region),
        ("read-along", config.shortcuts.read_along),
        ("copy-region", config.shortcuts.copy_region),
        ("listen-clipboard", config.shortcuts.listen_clipboard),
    ];
    let mut lookup = HashMap::new();
    for (id, binding) in bindings {
        let Some((modifiers, key)) = shortcut_parts(&binding) else {
            continue;
        };
        let Some(symbol) = x11_keysym(key) else {
            tracing::warn!(%binding,"unsupported X11 shortcut key");
            continue;
        };
        let Some(index) = mapping
            .keysyms
            .chunks(per_key)
            .position(|row| row.contains(&symbol))
        else {
            tracing::warn!(%binding,"shortcut key is not present in the X11 keymap");
            continue;
        };
        let code = first.saturating_add(index as u8);
        let mut mask = 0u16;
        for modifier in modifiers {
            mask |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ModMask::CONTROL.bits(),
                "alt" => ModMask::M1.bits(),
                "shift" => ModMask::SHIFT.bits(),
                "meta" | "super" | "win" => ModMask::M4.bits(),
                _ => 0,
            };
        }
        for locks in [
            0,
            ModMask::LOCK.bits(),
            ModMask::M2.bits(),
            ModMask::LOCK.bits() | ModMask::M2.bits(),
        ] {
            let _ = c
                .grab_key(
                    false,
                    root,
                    (mask | locks).into(),
                    code,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )?
                .check();
        }
        lookup.insert((code, mask), id.to_owned());
    }
    c.flush()?;
    tracing::info!(count = lookup.len(), "registered X11 global shortcuts");
    loop {
        let event = c.wait_for_event()?;
        if let x11rb::protocol::Event::KeyPress(KeyPressEvent { detail, state, .. }) = event {
            let modifiers = state.bits() & !(ModMask::LOCK.bits() | ModMask::M2.bits());
            if let Some(id) = lookup.get(&(detail, modifiers)) {
                let app = app.clone();
                let id = id.clone();
                std::thread::spawn(move || dispatch_shortcut(app, id));
            }
        }
    }
}
#[cfg(target_os = "linux")]
fn x11_keysym(key: &str) -> Option<u32> {
    match key.to_ascii_lowercase().as_str() {
        "escape" => Some(0xff1b),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        "return" | "enter" => Some(0xff0d),
        "tab" => Some(0xff09),
        "backspace" => Some(0xff08),
        "space" => Some(0x20),
        value if value.len() == 1 => Some(value.as_bytes()[0].to_ascii_lowercase() as u32),
        value if value.starts_with('f') => value[1..]
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0xffbd + number),
        _ => None,
    }
}

#[cfg(windows)]
fn windows_hotkeys(app: App) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };
    let config = app.inner.store.load()?;
    let bindings = [
        ("read-selection", config.shortcuts.read_selection),
        ("read-region", config.shortcuts.read_region),
        ("read-along", config.shortcuts.read_along),
        ("copy-region", config.shortcuts.copy_region),
        ("listen-clipboard", config.shortcuts.listen_clipboard),
    ];
    let mut actions = HashMap::new();
    for (index, (id, binding)) in bindings.into_iter().enumerate() {
        let Some((modifiers, key)) = shortcut_parts(&binding) else {
            continue;
        };
        let mut flags = 0;
        for modifier in modifiers {
            flags |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => MOD_CONTROL,
                "alt" => MOD_ALT,
                "shift" => MOD_SHIFT,
                "meta" | "super" | "win" => MOD_WIN,
                _ => 0,
            };
        }
        let Some(vk) = windows_virtual_key(key) else {
            tracing::warn!(%binding,"unsupported Windows shortcut key");
            continue;
        };
        if unsafe { RegisterHotKey(std::ptr::null_mut(), index as i32 + 1, flags, vk) } == 0 {
            tracing::warn!(%binding,"Windows shortcut registration failed; key may already be in use");
            continue;
        }
        actions.insert(index as i32 + 1, id.to_owned());
    }
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        if message.message == WM_HOTKEY {
            if let Some(id) = actions.get(&(message.wParam as i32)) {
                dispatch_shortcut(app.clone(), id.clone())
            }
        } else {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    app.handle(LocalRequest::Shutdown)?;
    Ok(())
}
#[cfg(windows)]
fn windows_virtual_key(key: &str) -> Option<u32> {
    match key.to_ascii_lowercase().as_str() {
        "escape" => Some(0x1b),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "return" | "enter" => Some(0x0d),
        "tab" => Some(0x09),
        "space" => Some(0x20),
        value if value.len() == 1 => Some(value.as_bytes()[0].to_ascii_uppercase() as u32),
        value if value.starts_with('f') => value[1..]
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=24).contains(n))
            .map(|n| 0x70 + n - 1),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
async fn portal_shortcuts(app: App) -> Result<()> {
    use ashpd::desktop::global_shortcuts::{
        Activated, BindShortcutsOptions, Deactivated, GlobalShortcuts, NewShortcut,
    };
    use ashpd::desktop::CreateSessionOptions;
    use futures_lite::StreamExt;
    ensure_host_desktop_entry()?;
    let portal = GlobalShortcuts::new().await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let config = app.inner.store.load()?;
    let configured = [
        (
            "read-selection",
            "Read selected text",
            config.shortcuts.read_selection,
        ),
        (
            "read-region",
            "Read screen region",
            config.shortcuts.read_region,
        ),
        (
            "read-along",
            "Start read-along",
            config.shortcuts.read_along,
        ),
        (
            "copy-region",
            "Copy screen text",
            config.shortcuts.copy_region,
        ),
        (
            "listen-clipboard",
            "Toggle clipboard listening",
            config.shortcuts.listen_clipboard,
        ),
    ];
    let shortcuts = configured
        .iter()
        .map(|(id, description, binding)| {
            let shortcut = NewShortcut::new(*id, *description);
            if let Some(trigger) = xdg_trigger(binding) {
                shortcut.preferred_trigger(Some(trigger.as_str()))
            } else {
                shortcut
            }
        })
        .collect::<Vec<_>>();
    let request = portal
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await?;
    let _bound = request.response()?;
    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;
    enum PortalEvent {
        Activated(Option<Activated>),
        Deactivated(Option<Deactivated>),
    }
    let mut pending_selection = None;
    loop {
        let event = futures_lite::future::race(
            async { PortalEvent::Activated(activated.next().await) },
            async { PortalEvent::Deactivated(deactivated.next().await) },
        )
        .await;
        match event {
            PortalEvent::Activated(Some(event)) => {
                let id = event.shortcut_id().to_owned();
                if id == "read-selection" {
                    pending_selection = Some(app.begin_read_intent_with_pending(true)?);
                    app.show_hud();
                } else {
                    let app = app.clone();
                    std::thread::spawn(move || dispatch_shortcut(app, id));
                }
            }
            PortalEvent::Deactivated(Some(event)) if event.shortcut_id() == "read-selection" => {
                if let Some(generation) = pending_selection.take() {
                    let app = app.clone();
                    std::thread::spawn(move || {
                        if app.inner.generation.accepts(generation) {
                            if let Err(error) = read_selection_hotkey_at(&app, generation) {
                                tracing::warn!(%error, "selection copy failed");
                            }
                        }
                    });
                }
            }
            PortalEvent::Activated(None) | PortalEvent::Deactivated(None) => {
                bail!("global shortcut portal signal stream ended")
            }
            PortalEvent::Deactivated(Some(_)) => {}
        }
    }
}
#[cfg(target_os = "linux")]
fn ensure_host_desktop_entry() -> Result<()> {
    let root = dirs::data_local_dir().context("locating XDG data directory for desktop entry")?;
    let apps = root.join("applications");
    std::fs::create_dir_all(&apps)?;
    let path = apps.join("io.github.yosef0H4.simple_stt.desktop");
    if !path.exists() {
        let ctl = sibling("readerctl")?;
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName=Simple STT\nComment=Local dictation, read aloud, and screen text\nExec=\"{}\" read-selection\nIcon=simple-stt\nTerminal=false\nCategories=Utility;Accessibility;\n",
            ctl.display()
        );
        std::fs::write(path, desktop)?;
        let _ = Command::new("update-desktop-database")
            .arg(&apps)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn read_selection_hotkey(app: &App) -> Result<()> {
    let generation = app.begin_read_intent_with_pending(true)?;
    app.show_hud();
    read_selection_hotkey_at(app, generation)
}
#[cfg(target_os = "linux")]
fn read_selection_hotkey_at(app: &App, generation: u64) -> Result<()> {
    let repeat_revision = app.begin_repeat_action("selection");
    let _region = app
        .inner
        .region_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    app.inner
        .activity
        .lock()
        .unwrap()
        .set(generation, "acquiring_selection");
    let _clipboard_suppression = app.suppress_clipboard_watcher();
    let result = (|| {
        let config = app.inner.store.load()?;
        let text = read_selection_linux(app, generation, &config)?;
        anyhow::ensure!(
            app.inner.generation.accepts(generation),
            "selection read was canceled"
        );
        app.start_text_at("selection".into(), text.clone(), generation)?;
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Text {
                source: "selection".into(),
                text: text.clone(),
            },
        );
        Ok(())
    })();
    if let Err(error) = &result {
        app.report_intent_failure(generation, "Selection read failed", error, false);
    }
    result
}
#[cfg(windows)]
fn read_selection_hotkey(app: &App) -> Result<()> {
    let generation = app.begin_read_intent_with_pending(true)?;
    app.show_hud();
    read_selection_hotkey_at(app, generation)
}
#[cfg(windows)]
fn read_selection_hotkey_at(app: &App, generation: u64) -> Result<()> {
    let repeat_revision = app.begin_repeat_action("selection");
    let _region = app
        .inner
        .region_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    app.inner
        .activity
        .lock()
        .unwrap()
        .set(generation, "acquiring_selection");
    let _clipboard_suppression = app.suppress_clipboard_watcher();
    let result = (|| {
        let config = app.inner.store.load()?;
        let text = windows_copy_selection(app, generation, &config)?;
        anyhow::ensure!(
            app.inner.generation.accepts(generation),
            "selection read was canceled"
        );
        app.start_text_at("selection".into(), text.clone(), generation)?;
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Text {
                source: "selection".into(),
                text,
            },
        );
        Ok(())
    })();
    if let Err(error) = &result {
        app.report_intent_failure(generation, "Selection read failed", error, false);
    }
    result
}
#[cfg(not(any(target_os = "linux", windows)))]
fn read_selection_hotkey(_app: &App) -> Result<()> {
    bail!("selection reading is not available on this platform")
}
#[cfg(not(any(target_os = "linux", windows)))]
fn read_selection_hotkey_at(_app: &App, _generation: u64) -> Result<()> {
    bail!("selection reading is not available on this platform")
}
/// End picker prewarm while excluding a concurrent cache publication.
fn end_ocr_prewarm_phase(
    intent_gate: &Mutex<()>,
    epoch: &AtomicU64,
    current: impl FnOnce() -> bool,
) -> Result<()> {
    let _intent = intent_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    anyhow::ensure!(current(), "OCR request was canceled");
    epoch.fetch_add(1, Ordering::AcqRel);
    Ok(())
}

// OCR copy has its own lifetime and never advances the speech generation.
fn commit_current_ocr<T>(
    intent_gate: &Mutex<()>,
    current: impl FnOnce() -> bool,
    commit: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _intent = intent_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    anyhow::ensure!(current(), "OCR request was canceled");
    commit()
}

#[cfg(any(target_os = "linux", windows))]
fn copy_region_with_target(
    app: &App,
    epoch: u64,
    language: Option<String>,
    target: read_along_capture::Target,
) -> Result<()> {
    copy_region_inner(app, epoch, language, Some(target), None)
}

#[cfg(any(target_os = "linux", windows))]
fn copy_region_inner(
    app: &App,
    epoch: u64,
    language: Option<String>,
    saved_target: Option<read_along_capture::Target>,
    repeat_revision: Option<u64>,
) -> Result<()> {
    let current = || {
        app.inner.copy_epoch.load(Ordering::Acquire) == epoch
            && !app.inner.shutdown.load(Ordering::Acquire)
    };
    let directory = data_dir().join("capture");
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut image = directory.join(format!("{}.png", Uuid::new_v4()));
    let config = app
        .inner
        .store
        .load()?
        .routed(reader_core::routing::Feature::Ocr, language.as_deref())?;
    // Copy has its own cancellation epoch and never prewarms a voice worker.
    let prewarm_active = Arc::new(AtomicBool::new(true));
    let prewarm_flag = Arc::clone(&prewarm_active);
    let prewarm_marker = Arc::new(AtomicU64::new(0));
    let worker_marker = Arc::clone(&prewarm_marker);
    let prewarm_app = app.clone();
    let prewarm_config = config.clone();
    std::thread::spawn(move || {
        let current = || {
            prewarm_flag.load(Ordering::Acquire)
                && prewarm_app.inner.copy_epoch.load(Ordering::Acquire) == epoch
                && !prewarm_app.inner.shutdown.load(Ordering::Acquire)
        };
        if let Err(error) = prepare_ocr(
            &prewarm_app,
            &prewarm_config,
            20_000_000,
            true,
            true,
            Some(&worker_marker),
            current,
        ) {
            tracing::debug!(%error, "OCR copy prewarm ended");
        }
    });
    {
        let mut activity = app.inner.ocr_activity.lock().unwrap();
        activity.begin(epoch);
        activity.set(epoch, "selecting_region");
    }
    let result = (|| -> Result<()> {
        {
            let _region = app
                .inner
                .region_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            anyhow::ensure!(current(), "OCR copy canceled");
            #[cfg(target_os = "linux")]
            {
                let selected = match saved_target {
                    Some(target) => Some((target.clone(), target.capture(&current)?)),
                    None if read_along_capture::Target::supported_on_current_desktop() => {
                        Some(read_along_capture::Target::select(current)?)
                    }
                    None => None,
                };
                if let Some((target, captured)) = selected {
                    if let Some(revision) = repeat_revision {
                        app.complete_repeat_action(
                            revision,
                            repeat::RepeatPayload::Region {
                                action: repeat::RegionAction::Copy,
                                target,
                            },
                        );
                    }
                    image = captured;
                } else {
                    if let Some(revision) = repeat_revision {
                        app.complete_repeat_action(
                            revision,
                            repeat::RepeatPayload::Unavailable {
                                reason: "this desktop cannot bind a screenshot region to a specific window".into(),
                            },
                        );
                    }
                    app.set_notice(
                        "Repeat is unavailable for this screen capture on this desktop".into(),
                        Duration::from_secs(5),
                    );
                    region_capture::capture_region(&image, current)?;
                }
            }
            #[cfg(windows)]
            {
                let selected = match saved_target {
                    Some(target) => Some((target.clone(), target.capture(&current)?)),
                    None if read_along_capture::Target::supported_on_current_desktop() => {
                        Some(read_along_capture::Target::select(current)?)
                    }
                    None => None,
                };
                if let Some((target, captured)) = selected {
                    if let Some(revision) = repeat_revision {
                        app.complete_repeat_action(
                            revision,
                            repeat::RepeatPayload::Region {
                                action: repeat::RegionAction::Copy,
                                target,
                            },
                        );
                    }
                    image = captured;
                } else {
                    if let Some(revision) = repeat_revision {
                        app.complete_repeat_action(
                            revision,
                            repeat::RepeatPayload::Unavailable {
                                reason: "this desktop cannot bind a screenshot region to a specific window".into(),
                            },
                        );
                    }
                    app.set_notice(
                        "Repeat is unavailable for this screen capture on this desktop".into(),
                        Duration::from_secs(5),
                    );
                    let selector =
                        directory.join(format!("region-selector-{}.ps1", Uuid::new_v4()));
                    std::fs::write(
                        &selector,
                        include_str!("../../../assets/windows-region-selector.ps1"),
                    )?;
                    let capture = (|| -> Result<()> {
                        let mut child = Command::new("powershell.exe")
                            .args([
                                "-NoLogo",
                                "-NoProfile",
                                "-STA",
                                "-ExecutionPolicy",
                                "Bypass",
                                "-File",
                            ])
                            .arg(&selector)
                            .arg(&image)
                            .spawn()?;
                        let started = Instant::now();
                        loop {
                            if !current() || started.elapsed() > Duration::from_secs(180) {
                                let _ = child.kill();
                                let _ = child.wait();
                                bail!("screen-region capture canceled");
                            }
                            if let Some(status) = child.try_wait()? {
                                anyhow::ensure!(
                                    status.success() && image.is_file(),
                                    "screen-region capture failed"
                                );
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        Ok(())
                    })();
                    let _ = std::fs::remove_file(selector);
                    capture?;
                }
            }
        }
        anyhow::ensure!(current(), "OCR copy canceled");
        {
            let mut activity = app.inner.ocr_activity.lock().unwrap();
            if activity.generation == epoch {
                activity.language = Some(config.ocr.language.clone());
                activity.model = config
                    .ocr
                    .model_dir
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.runtime = config
                    .ocr
                    .runtime_dir
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned());
                activity.set(epoch, "recognizing");
            }
        }
        let text = run_ocr(app, &config, &image, 20_000_000, true, current, None)?;
        anyhow::ensure!(current(), "OCR copy canceled");
        // The guard rebaselines while suppression is still held, so delayed listener
        // events see the copied text as the baseline instead of starting speech.
        let _region = app
            .inner
            .region_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        commit_current_ocr(&app.inner.intent_gate, current, || {
            let _suppression = app.suppress_clipboard_watcher();
            app.inner.ocr_activity.lock().unwrap().set(epoch, "copying");
            copy_ocr_text(&text)?;
            app.set_notice("Text copied".into(), Duration::from_secs(3));
            app.show_hud();
            Ok(())
        })
    })();
    finish_copy_prewarm(
        &app.inner,
        &prewarm_active,
        &prewarm_marker,
        config.ocr.idle_timeout_secs,
    );
    let _ = std::fs::remove_file(image);
    app.inner.ocr_activity.lock().unwrap().set(epoch, "idle");
    result
}
#[cfg(any(target_os = "linux", windows))]
fn start_region_prewarm(app: &App, generation: u64) -> Result<()> {
    let config = app.inner.store.load()?;
    let (capture_epoch, ocr_prewarm_epoch) = {
        let _intent = app
            .inner
            .intent_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        anyhow::ensure!(
            app.inner.generation.accepts(generation),
            "screen capture was canceled"
        );
        (
            app.inner.capture_epoch.fetch_add(1, Ordering::AcqRel) + 1,
            app.inner.ocr_prewarm_epoch.fetch_add(1, Ordering::AcqRel) + 1,
        )
    };
    let warm_config =
        reading_hint::prewarm(&config, app.inner.route_language.lock().unwrap().as_deref());
    let speculative_epoch = app.inner.speculative_epoch.load(Ordering::Acquire);
    let ocr_config = config.routed(
        reader_core::routing::Feature::Ocr,
        app.inner.route_language.lock().unwrap().as_deref(),
    );
    let ocr_warm = app.clone();
    std::thread::spawn(move || {
        if let Ok(config) = ocr_config {
            if ocr_warm.inner.generation.accepts(generation)
                && ocr_warm.inner.capture_epoch.load(Ordering::Acquire) == capture_epoch
                && ocr_warm.inner.ocr_prewarm_epoch.load(Ordering::Acquire) == ocr_prewarm_epoch
            {
                if let Err(error) =
                    prepare_ocr(&ocr_warm, &config, 20_000_000, false, true, None, || {
                        ocr_warm.inner.generation.accepts(generation)
                            && ocr_warm.inner.capture_epoch.load(Ordering::Acquire) == capture_epoch
                            && ocr_warm.inner.ocr_prewarm_epoch.load(Ordering::Acquire)
                                == ocr_prewarm_epoch
                    })
                {
                    tracing::debug!(%error, "OCR prewarm failed");
                }
            }
        }
    });
    let warm = app.clone();
    std::thread::spawn(move || {
        let Ok(warm_config) = warm_config else {
            return;
        };
        let preparation = warm.inner.tts.prepare_with_cancel(
            &warm_config,
            Some(&warm.inner.intent_gate),
            Some(generation),
            || {
                warm.inner.generation.accepts(generation)
                    && warm.inner.speculative_epoch.load(Ordering::Acquire) == speculative_epoch
                    && warm.inner.capture_epoch.load(Ordering::Acquire) == capture_epoch
            },
        );
        if let Err(error) = preparation {
            if !warm.inner.generation.accepts(generation)
                || warm.inner.speculative_epoch.load(Ordering::Acquire) != speculative_epoch
                || warm.inner.capture_epoch.load(Ordering::Acquire) != capture_epoch
            {
                tracing::debug!(%error, "voice preparation canceled with screen capture");
                warm.schedule_idle_lease_if_current();
                return;
            }
            tracing::warn!(%error,"voice preparation failed while selecting screen region");
            warm.report_intent_failure(generation, "Voice preparation failed", &error, true);
        } else if warm.inner.generation.accepts(generation)
            && warm.inner.capture_epoch.load(Ordering::Acquire) == capture_epoch
        {
            warm.schedule_unload(warm_config.playback.keep_warm_seconds, generation);
        } else {
            warm.schedule_idle_lease_if_current();
        }
    });
    Ok(())
}
#[cfg(target_os = "linux")]
fn read_region_hotkey(app: &App) -> Result<()> {
    let generation = app.begin_read_intent_with_pending(true)?;
    app.show_hud();
    read_region_hotkey_at(app, generation)
}
#[cfg(target_os = "linux")]
fn read_region_hotkey_at(app: &App, generation: u64) -> Result<()> {
    let repeat_revision = app.begin_repeat_action("read-region");
    app.inner
        .activity
        .lock()
        .unwrap()
        .set(generation, "selecting_region");
    let result = read_region_hotkey_inner(app, generation, repeat_revision);
    if let Err(error) = &result {
        app.report_intent_failure(generation, "Screen read failed", error, false);
    }
    result
}
#[cfg(target_os = "linux")]
fn read_region_hotkey_inner(app: &App, generation: u64, repeat_revision: u64) -> Result<()> {
    anyhow::ensure!(
        app.inner.generation.accepts(generation),
        "screen capture was canceled"
    );
    start_region_prewarm(app, generation)?;
    let current =
        || app.inner.generation.accepts(generation) && !app.inner.shutdown.load(Ordering::Acquire);
    let _region = app
        .inner
        .region_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let selected = if read_along_capture::Target::supported_on_current_desktop() {
        Some(read_along_capture::Target::select(current)?)
    } else {
        None
    };
    let path = if let Some((target, image)) = selected {
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Region {
                action: repeat::RegionAction::Read,
                target,
            },
        );
        image
    } else {
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Unavailable {
                reason: "this desktop cannot bind a screenshot region to a specific window".into(),
            },
        );
        app.set_notice(
            "Repeat is unavailable for this screen capture on this desktop".into(),
            Duration::from_secs(5),
        );
        let dir = data_dir().join("capture");
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = dir.join(format!("{}.png", Uuid::new_v4()));
        region_capture::capture_region(&path, current)?;
        path
    };
    let _image_cleanup = privacy::TemporaryFile(path.clone());
    anyhow::ensure!(current(), "screen capture was canceled");
    app.read_image_at(path.to_str().context("invalid image path")?, generation)?;
    Ok(())
}
#[cfg(windows)]
fn read_region_hotkey(app: &App) -> Result<()> {
    let generation = app.begin_read_intent_with_pending(true)?;
    app.show_hud();
    read_region_hotkey_at(app, generation)
}
#[cfg(windows)]
fn read_region_hotkey_at(app: &App, generation: u64) -> Result<()> {
    let repeat_revision = app.begin_repeat_action("read-region");
    app.inner
        .activity
        .lock()
        .unwrap()
        .set(generation, "selecting_region");
    let result = read_region_hotkey_inner(app, generation, repeat_revision);
    if let Err(error) = &result {
        app.report_intent_failure(generation, "Screen read failed", error, false);
    }
    result
}
#[cfg(windows)]
fn read_region_hotkey_inner(app: &App, generation: u64, repeat_revision: u64) -> Result<()> {
    let _region = app
        .inner
        .region_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    anyhow::ensure!(
        app.inner.generation.accepts(generation),
        "screen capture was canceled"
    );
    start_region_prewarm(app, generation)?;
    let directory = data_dir().join("capture");
    std::fs::create_dir_all(&directory)?;
    let current =
        || app.inner.generation.accepts(generation) && !app.inner.shutdown.load(Ordering::Acquire);
    let _region = app
        .inner
        .region_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = if read_along_capture::Target::supported_on_current_desktop() {
        let (target, image) = read_along_capture::Target::select(current)?;
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Region {
                action: repeat::RegionAction::Read,
                target,
            },
        );
        image
    } else {
        app.complete_repeat_action(
            repeat_revision,
            repeat::RepeatPayload::Unavailable {
                reason: "this desktop cannot bind a screenshot region to a specific window".into(),
            },
        );
        app.set_notice(
            "Repeat is unavailable for this screen capture on this desktop".into(),
            Duration::from_secs(5),
        );
        let selector = directory.join("region-selector.ps1");
        std::fs::write(
            &selector,
            include_str!("../../../assets/windows-region-selector.ps1"),
        )?;
        let image = directory.join(format!("{}.png", Uuid::new_v4()));
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-STA",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&selector)
            .arg(&image)
            .spawn()
            .context("starting the Windows screen-region selector")?;
        let incarnation = Uuid::new_v4().to_string();
        let identity = ChildKill::new(child.id(), incarnation.clone())?;
        if let Err(error) = app.publish_capture(generation, identity, "screen selector") {
            let _ = child.wait();
            let _ = std::fs::remove_file(selector);
            let _ = std::fs::remove_file(&image);
            return Err(error);
        }
        let status = child
            .wait()
            .context("waiting for the Windows screen-region selector")?;
        {
            let mut capture = app
                .inner
                .capture_kill
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if capture
                .as_ref()
                .is_some_and(|identity| identity.incarnation == incarnation)
            {
                *capture = None;
            }
        }
        let _ = std::fs::remove_file(selector);
        if !current() {
            let _ = std::fs::remove_file(&image);
            bail!("screen capture was canceled");
        }
        if !status.success() || !image.is_file() {
            let _ = std::fs::remove_file(image);
            bail!("screen-region capture was canceled or failed");
        }
        image
    };
    let _image_cleanup = privacy::TemporaryFile(path.clone());
    anyhow::ensure!(current(), "screen capture was canceled");
    app.read_image_at(
        path.to_str().context("invalid captured image path")?,
        generation,
    )?;
    Ok(())
}
#[cfg(not(any(target_os = "linux", windows)))]
fn read_region_hotkey(_app: &App) -> Result<()> {
    bail!("screen-region capture is not available on this platform")
}
#[cfg(not(any(target_os = "linux", windows)))]
fn read_region_hotkey_at(_app: &App, _generation: u64) -> Result<()> {
    bail!("screen-region capture is not available on this platform")
}

#[cfg(any(target_os = "linux", windows))]
fn run_owned_output(
    app: &App,
    generation: u64,
    mut command: Command,
    label: &str,
    timeout: Duration,
) -> Result<Output> {
    use std::io::Read;
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {label}"))?;
    let incarnation = Uuid::new_v4().to_string();
    let identity = ChildKill::new(child.id(), incarnation.clone())?;
    let lifecycle = app
        .inner
        .pause_lifecycle_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut capture = app
        .inner
        .capture_kill
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !app.inner.generation.accepts(generation) || capture.is_some() {
        drop(capture);
        drop(lifecycle);
        identity.terminate(label)?;
        let _ = child.wait();
        bail!("{label} was canceled or superseded");
    }
    *capture = Some(identity);
    drop(capture);
    drop(lifecycle);
    let mut stdout = child
        .stdout
        .take()
        .context("selection helper stdout missing")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout
            .by_ref()
            .take((MAX_TEXT_BYTES + 1) as u64)
            .read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        let current = app.inner.generation.accepts(generation);
        if !current || Instant::now() >= deadline {
            let _ = app.kill_capture_if(&incarnation);
            let _ = child.wait();
            let _ = reader.join();
            bail!(if current {
                format!("{label} timed out")
            } else {
                format!("{label} was canceled")
            });
        }
        if let Some(status) = child.try_wait().context("waiting for selection helper")? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    {
        let mut guard = app
            .inner
            .capture_kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard
            .as_ref()
            .is_some_and(|entry| entry.incarnation == incarnation)
        {
            *guard = None;
        }
    }
    let stdout = reader
        .join()
        .map_err(|_| anyhow::anyhow!("{label} output reader failed"))?;
    anyhow::ensure!(
        stdout.len() <= MAX_TEXT_BYTES,
        "{label} output is too large"
    );
    Ok(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

#[cfg(target_os = "linux")]
fn selection_helper() -> Result<PathBuf> {
    let installed = std::env::current_exe()?
        .parent()
        .context("executable has no parent")?
        .join("linux-fast-paste");
    if installed.is_file() {
        return Ok(installed);
    }
    // Isolated development builds; installed packages always use the sibling helper.
    let development =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources/bin/linux-fast-paste");
    anyhow::ensure!(
        development.is_file(),
        "native selection helper is not installed"
    );
    Ok(development)
}

#[cfg(target_os = "linux")]
fn read_selection_linux(app: &App, generation: u64, config: &AppConfig) -> Result<String> {
    let app_id = if config.selection_copy_overrides.is_empty() {
        None
    } else {
        active_selection_app(app, generation).ok().flatten()
    };
    if let Ok(helper) = selection_helper() {
        let mut command = Command::new(helper);
        command.arg("--read-selection");
        let output = run_owned_output(
            app,
            generation,
            command,
            "reading selected text directly",
            Duration::from_secs(2),
        )?;
        if output.status.success() {
            let text = String::from_utf8(output.stdout).context("selected text is not UTF-8")?;
            anyhow::ensure!(!text.trim().is_empty(), "there is no selected text");
            return Ok(text);
        }
        anyhow::ensure!(output.status.code() != Some(2), "there is no selected text");
    }
    anyhow::ensure!(
        config.selection_copy.allow_clipboard_copy,
        "selected text is unavailable directly; copy it manually or enable shortcut copy in Settings"
    );
    wait_for_copy_modifiers(app, generation)?;
    let mut watch = selection_watch::SelectionWatch::start()?;
    let before = watch.revision()?;
    let shortcut = config.selection_copy_shortcut(app_id.as_deref());
    let key = match shortcut {
        SelectionCopyShortcut::CtrlInsert => "Ctrl+Insert",
        SelectionCopyShortcut::CtrlC => "Ctrl+C",
        SelectionCopyShortcut::CtrlShiftC => "Ctrl+Shift+C",
    };
    portal_input::run(portal_input::Action::Key { key: key.into() }, &|| {
        app.inner.generation.accepts(generation)
    })?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        anyhow::ensure!(
            app.inner.generation.accepts(generation),
            "selection read was canceled"
        );
        let revision = watch.revision()?;
        if revision != before {
            let text = read_desktop_clipboard(app, generation)?;
            anyhow::ensure!(
                watch.revision()? == revision,
                "clipboard changed again; select the text and retry"
            );
            if config.selection_copy.show_clipboard_notice {
                app.set_notice("reader.clipboard_replaced".into(), Duration::from_secs(3));
            }
            return Ok(text);
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "the selected app did not copy text; try another copy shortcut in Settings"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn wait_until_modifiers_released(
    current: impl Fn() -> bool,
    mut held: impl FnMut() -> Result<bool>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut released = None;
    loop {
        anyhow::ensure!(current(), "selection read was canceled");
        anyhow::ensure!(
            Instant::now() < deadline,
            "release the copy shortcut modifiers and retry"
        );
        if held()? {
            released = None;
        } else if released.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(40) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
fn wait_for_copy_modifiers(app: &App, generation: u64) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        use x11rb::{
            connection::Connection as _,
            protocol::xproto::{ConnectionExt as _, KeyButMask},
        };
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        return wait_until_modifiers_released(
            || app.inner.generation.accepts(generation),
            || {
                Ok(connection.query_pointer(root)?.reply()?.mask.intersects(
                    KeyButMask::CONTROL | KeyButMask::SHIFT | KeyButMask::MOD1 | KeyButMask::MOD4,
                ))
            },
            Duration::from_secs(2),
        );
    }
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    const BITMAP_BYTES: usize = 96;
    const EV_KEY: usize = 1;
    const KEY_A: usize = 30;
    const KEY_Z: usize = 44;
    const MODIFIERS: &[usize] = &[29, 97, 56, 100, 42, 54, 125, 126];

    fn request(event_type: usize, number: usize, size: usize) -> libc::c_ulong {
        ((2u64 << 30)
            | ((size as u64) << 16)
            | ((u64::from(b'E')) << 8)
            | (0x20 + event_type + number) as u64) as libc::c_ulong
    }
    fn bit_is_set(bits: &[u8], code: usize) -> bool {
        bits.get(code / 8)
            .is_some_and(|byte| byte & (1 << (code % 8)) != 0)
    }
    fn bitmap(fd: libc::c_int, ioctl_request: libc::c_ulong) -> Result<[u8; BITMAP_BYTES]> {
        let mut bits = [0u8; BITMAP_BYTES];
        // Query capability/state only: no input events are read, stored, or logged.
        let result = unsafe { libc::ioctl(fd, ioctl_request, bits.as_mut_ptr()) };
        anyhow::ensure!(result >= 0, "could not query keyboard key state");
        Ok(bits)
    }

    let mut keyboards = Vec::new();
    let entries = std::fs::read_dir("/dev/input").context("opening Linux input devices")?;
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        let Ok(device) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(entry.path())
        else {
            continue;
        };
        let fd = device.as_raw_fd();
        let Ok(event_types) = bitmap(fd, request(0, 0, 8)) else {
            continue;
        };
        if !bit_is_set(&event_types, EV_KEY) {
            continue;
        }
        let Ok(keys) = bitmap(fd, request(EV_KEY, 0, BITMAP_BYTES)) else {
            continue;
        };
        if bit_is_set(&keys, KEY_A) && bit_is_set(&keys, KEY_Z) {
            keyboards.push(device);
        }
    }
    anyhow::ensure!(
        !keyboards.is_empty(),
        "cannot verify modifier release: no readable keyboard event devices"
    );

    wait_until_modifiers_released(
        || app.inner.generation.accepts(generation),
        || {
            let mut held = false;
            for device in &keyboards {
                let state = bitmap(
                    device.as_raw_fd(),
                    0x8000_0000u64 | ((BITMAP_BYTES as u64) << 16) | (u64::from(b'E') << 8) | 0x18,
                )
                .context("checking whether copy modifiers are still held")?;
                held |= MODIFIERS.iter().any(|code| bit_is_set(&state, *code));
            }
            Ok(held)
        },
        Duration::from_secs(2),
    )
}

#[cfg(windows)]
fn wait_for_copy_modifiers(app: &App, generation: u64) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    const MODIFIERS: [i32; 5] = [0x11, 0x12, 0x10, 0x5b, 0x5c];
    wait_until_modifiers_released(
        || app.inner.generation.accepts(generation),
        || {
            Ok(MODIFIERS
                .iter()
                .any(|key| unsafe { GetAsyncKeyState(*key) < 0 }))
        },
        Duration::from_secs(2),
    )
}

#[cfg(target_os = "linux")]
fn active_selection_app(app: &App, generation: u64) -> Result<Option<String>> {
    let command = if let Ok(helper) = selection_helper() {
        let mut command = Command::new(helper);
        command.arg("--active-app");
        command
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let Some(helper) = active_app_query_path()? else {
            return Ok(None);
        };
        Command::new(helper)
    } else {
        let mut command = Command::new("xdotool");
        command.args(["getactivewindow", "getwindowclassname"]);
        command
    };
    let output = run_owned_output(
        app,
        generation,
        command,
        "active application query",
        Duration::from_secs(2),
    );
    let Ok(output) = output else { return Ok(None) };
    if !output.status.success() {
        return Ok(None);
    }
    let app_id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!app_id.is_empty() && app_id.len() <= 256).then_some(app_id))
}

#[cfg(target_os = "linux")]
fn active_app_query_path() -> Result<Option<PathBuf>> {
    if let Some(path) = std::env::var_os("SIMPLE_TTS_ACTIVE_APP_QUERY") {
        return Ok(Some(PathBuf::from(path)));
    }
    #[cfg(atspi_helper)]
    {
        install_active_app_helper().map(Some)
    }
    #[cfg(not(atspi_helper))]
    {
        Ok(None)
    }
}

#[cfg(all(target_os = "linux", atspi_helper))]
fn install_active_app_helper() -> Result<PathBuf> {
    let directory = data_dir();
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("active-app-query");
    let expected = include_bytes!(env!("SIMPLE_TTS_ACTIVE_APP_HELPER"));
    if !std::fs::read(&path).is_ok_and(|installed| installed == expected) {
        let temporary = directory.join(format!("active-app-query-{}.tmp", Uuid::new_v4()));
        std::fs::write(&temporary, expected)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::rename(temporary, &path)?;
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn read_desktop_clipboard(app: &App, generation: u64) -> Result<String> {
    let command = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut command = Command::new("wl-paste");
        command.args(["--no-newline", "--type", "text/plain;charset=utf-8"]);
        command
    } else {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard", "-o"]);
        command
    };
    let output = run_owned_output(
        app,
        generation,
        command,
        "reading copied selection",
        Duration::from_secs(2),
    )?;
    anyhow::ensure!(
        output.status.success(),
        "selection clipboard is unavailable"
    );
    let text = String::from_utf8(output.stdout).context("selected text is not UTF-8")?;
    anyhow::ensure!(!text.trim().is_empty(), "there is no selected text");
    Ok(text)
}

#[cfg(windows)]
fn windows_copy_selection(app: &App, generation: u64, config: &AppConfig) -> Result<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    wait_for_copy_modifiers(app, generation)?;
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class SimpleTtsClipboardGuard { [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId); [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber(); }'
$settings = $env:SIMPLE_TTS_COPY_SETTINGS | ConvertFrom-Json
$backend = $settings.backend
$shortcut = $settings.shortcut
$foreground = [SimpleTtsClipboardGuard]::GetForegroundWindow()
[uint32]$foregroundPid = 0
[void][SimpleTtsClipboardGuard]::GetWindowThreadProcessId($foreground, [ref]$foregroundPid)
$processName = (Get-Process -Id $foregroundPid -ErrorAction SilentlyContinue).ProcessName
foreach ($rule in ($env:SIMPLE_TTS_COPY_OVERRIDES | ConvertFrom-Json)) { if ($rule.app_id -and $processName -and ([string]::Equals($rule.app_id, $processName, [StringComparison]::OrdinalIgnoreCase) -or [string]::Equals($rule.app_id, "$processName.exe", [StringComparison]::OrdinalIgnoreCase))) { $shortcut = $rule.shortcut; if ($rule.backend) { $backend = $rule.backend }; break } }
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
if ($backend -eq 'auto' -or $backend -eq 'accessibility') {
  $selection = $null
  try {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($focused.Current.IsPassword) { throw 'Password selection is unavailable.' }
    $pattern = $focused.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
    $ranges = $pattern.GetSelection()
    if ($ranges.Count -eq 1) { $selection = $ranges[0].GetText(1048577) }
  } catch { }
  if ($null -ne $selection) {
    if ([string]::IsNullOrWhiteSpace($selection)) { throw 'There is no selected text.' }
    [Console]::Write((@{text=$selection; clipboard_replaced=$false} | ConvertTo-Json -Compress)); exit
  }
  if ($backend -eq 'accessibility') { throw 'Selected text is unavailable through accessibility.' }
}
if (!$settings.allow_clipboard_copy) { throw 'Copy the selection manually or enable shortcut copy in Settings.' }
$before = [SimpleTtsClipboardGuard]::GetClipboardSequenceNumber()
$key = switch ($shortcut) { 'ctrl_c' { '^c' } 'ctrl_shift_c' { '^+c' } default { '^{INSERT}' } }
[System.Windows.Forms.SendKeys]::SendWait($key)
$deadline = [DateTime]::UtcNow.AddSeconds(2)
do {
  $copiedAt = [SimpleTtsClipboardGuard]::GetClipboardSequenceNumber()
  if ($copiedAt -ne $before) { break }
  Start-Sleep -Milliseconds 15
} while ([DateTime]::UtcNow -lt $deadline)
if ($copiedAt -eq $before) { throw 'The selected app did not update the clipboard.' }
$s = [System.Windows.Forms.Clipboard]::GetText()
if ([SimpleTtsClipboardGuard]::GetClipboardSequenceNumber() -ne $copiedAt) { throw 'Clipboard changed again; retry.' }
if ([string]::IsNullOrWhiteSpace($s)) { throw 'There is no selected text.' }
[Console]::Write((@{text=$s; clipboard_replaced=$true} | ConvertTo-Json -Compress))
"#;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoLogo", "-NoProfile", "-STA", "-Command", script])
        .env(
            "SIMPLE_TTS_COPY_OVERRIDES",
            serde_json::to_string(&config.selection_copy_overrides)?,
        )
        .env(
            "SIMPLE_TTS_COPY_SETTINGS",
            serde_json::to_string(&config.selection_copy)?,
        )
        .creation_flags(CREATE_NO_WINDOW);
    let output = run_owned_output(
        app,
        generation,
        command,
        "reading selected text",
        Duration::from_secs(6),
    )?;
    anyhow::ensure!(
        output.status.success(),
        "Windows selection copy failed; choose another copy method in Settings"
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let text = response["text"]
        .as_str()
        .context("selected text is missing")?
        .to_owned();
    anyhow::ensure!(!text.trim().is_empty(), "there is no selected text");
    anyhow::ensure!(text.len() <= MAX_TEXT_BYTES, "selected text is too large");
    if config.selection_copy.show_clipboard_notice && response["clipboard_replaced"] == true {
        app.set_notice("reader.clipboard_replaced".into(), Duration::from_secs(3));
    }
    Ok(text)
}

#[cfg(target_os = "linux")]
struct Tray {
    app: App,
}
#[cfg(target_os = "linux")]
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "simple-tts".into()
    }
    fn title(&self) -> String {
        "Simple TTS".into()
    }
    fn icon_name(&self) -> String {
        "audio-volume-high".into()
    }
    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};
        let app = self.app.clone();
        vec![
            StandardItem {
                label: "Abort reading".into(),
                icon_name: "process-stop".into(),
                activate: Box::new(|tray: &mut Self| {
                    let app = tray.app.clone();
                    std::thread::spawn(move || {
                        let _ = app.action_stop();
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Read selection".into(),
                icon_name: "edit-select-all".into(),
                activate: Box::new(move |_| {
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let _ = read_selection_hotkey(&app);
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Read screen region".into(),
                icon_name: "edit-select".into(),
                activate: Box::new(|tray: &mut Self| {
                    let app = tray.app.clone();
                    std::thread::spawn(move || {
                        let _ = read_region_hotkey(&app);
                    });
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Listen to clipboard".into(),
                checked: self
                    .app
                    .inner
                    .clipboard_listening
                    .load(Ordering::Acquire),
                activate: Box::new(|tray: &mut Self| {
                    let app = tray.app.clone();
                    std::thread::spawn(move || {
                        if let Err(error) = app.action_toggle_clipboard_listening() {
                            tracing::warn!(%error, "could not toggle clipboard listening from tray");
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings".into(),
                icon_name: "configure".into(),
                activate: Box::new(|_| {
                    if let Ok(path) = sibling("reader-settings") {
                        let _ = spawn_reaped(Command::new(path));
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.app.inner.shutdown.store(true, Ordering::Relaxed);
                    wake_unix_socket();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}
#[cfg(target_os = "linux")]
fn start_tray(app: App) {
    use ksni::blocking::TrayMethods;
    if let Ok(handle) = (Tray { app: app.clone() })
        .assume_sni_available(true)
        .spawn()
    {
        *app.inner
            .tray_handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

#[cfg(windows)]
fn windows_tray(app: App) {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    static TRAY_APP: OnceLock<App> = OnceLock::new();
    const TRAY_MESSAGE: u32 = WM_USER + 1;
    const ID_SELECTION: usize = 1;
    const ID_REGION: usize = 2;
    const ID_ABORT: usize = 3;
    const ID_SETTINGS: usize = 4;
    const ID_QUIT: usize = 5;
    const ID_LISTEN_CLIPBOARD: usize = 6;

    unsafe extern "system" fn tray_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> isize {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let app = TRAY_APP.get();
        if message == TRAY_MESSAGE
            && (lparam as u32 == WM_RBUTTONUP || lparam as u32 == WM_CONTEXTMENU)
        {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return 0;
            }
            for (id, text) in [
                (ID_ABORT, "Abort reading"),
                (ID_SELECTION, "Read selection"),
                (ID_REGION, "Read screen region"),
                (ID_LISTEN_CLIPBOARD, "Listen to clipboard"),
                (ID_SETTINGS, "Settings"),
                (ID_QUIT, "Quit"),
            ] {
                let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let checked = if id == ID_LISTEN_CLIPBOARD
                    && app.is_some_and(|app| app.inner.clipboard_listening.load(Ordering::Acquire))
                {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
                InsertMenuW(
                    menu,
                    u32::MAX,
                    MF_BYPOSITION | MF_STRING | checked,
                    id,
                    wide.as_ptr(),
                );
            }
            let mut point = std::mem::zeroed();
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
            let selected = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            DestroyMenu(menu);
            if let Some(app) = app {
                match selected as usize {
                    ID_ABORT => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = app.action_stop();
                        });
                    }
                    ID_SELECTION => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = read_selection_hotkey(&app);
                        });
                    }
                    ID_REGION => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = read_region_hotkey(&app);
                        });
                    }
                    ID_LISTEN_CLIPBOARD => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            if let Err(error) = app.action_toggle_clipboard_listening() {
                                tracing::warn!(%error,"could not toggle clipboard listening from tray");
                            }
                        });
                    }
                    ID_SETTINGS => {
                        if let Ok(path) = sibling("reader-settings") {
                            let _ = spawn_reaped(Command::new(path));
                        }
                    }
                    ID_QUIT => {
                        app.inner.shutdown.store(true, Ordering::Relaxed);
                        wake_pipe();
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }
            return 0;
        }
        if message == WM_POWERBROADCAST {
            let event = wparam as u32;
            if event == PBT_APMSUSPEND {
                if let Some(app) = app.cloned() {
                    std::thread::spawn(move || handle_power_transition(app, true));
                }
                return 1;
            }
            if event == PBT_APMRESUMEAUTOMATIC || event == PBT_APMRESUMESUSPEND {
                if let Some(app) = app.cloned() {
                    std::thread::spawn(move || handle_power_transition(app, false));
                }
                return 1;
            }
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    if TRAY_APP.set(app.clone()).is_err() {
        return;
    }
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class: Vec<u16> = "SimpleTtsTrayWindow"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let descriptor = WNDCLASSW {
            lpfnWndProc: Some(tray_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&descriptor) == 0 {
            tracing::error!("could not register Windows tray window");
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            tracing::error!("could not create Windows tray window");
            return;
        }
        let tip: Vec<u16> = "Simple TTS".encode_utf16().chain(Some(0)).collect();
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        for (dst, src) in data.szTip.iter_mut().zip(tip) {
            *dst = src;
        }
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            tracing::error!("could not add Simple TTS tray icon");
            DestroyWindow(hwnd);
            return;
        }
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
            if app.inner.shutdown.load(Ordering::Relaxed) {
                break;
            }
        }
        Shell_NotifyIconW(NIM_DELETE, &data);
        DestroyWindow(hwnd);
    }
}

fn run_server(store: ConfigStore) -> Result<()> {
    let _processing = privacy::ProcessingLease::acquire(&data_dir())?;
    anyhow::ensure!(
        !local_daemon_is_running(),
        "the desktop controller is already running; start the API server from Settings or use the tray server_start action"
    );
    let app = App::new(store);
    app.start_server()?;
    eprintln!("Authenticated OpenAI-compatible server is running; press Ctrl+C to stop.");
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = stopped.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    while !stopped.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    app.action("server_stop")?;
    Ok(())
}

#[cfg(unix)]
fn local_daemon_is_running() -> bool {
    std::os::unix::net::UnixStream::connect(data_dir().join("simple-tts.sock")).is_ok()
}

#[cfg(windows)]
fn local_daemon_is_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    let name = pipe_name();
    if unsafe { WaitNamedPipeW(name.as_ptr(), 100) } == 0 {
        return false;
    }
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0x8000_0000 | 0x4000_0000,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        false
    } else {
        unsafe { CloseHandle(handle) };
        true
    }
}

fn start_media_bridge() -> Result<MediaBridge> {
    let mut command = Command::new(sibling("simple-stt-ctl")?);
    command
        .arg("media-serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    start_media_bridge_command(command)
}

fn start_media_bridge_command(mut command: Command) -> Result<MediaBridge> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .context("starting persistent media transcription bridge")?;
    #[cfg(windows)]
    let job = match create_media_bridge_job(&mut child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let input = child
        .stdin
        .take()
        .context("media bridge stdin unavailable")?;
    let output = child
        .stdout
        .take()
        .context("media bridge stdout unavailable")?;
    let (requests, incoming) = mpsc::channel::<MediaBridgeRequest>();
    let worker = std::thread::Builder::new()
        .name("media-bridge-io".into())
        .spawn(move || {
            let mut input = input;
            let mut output = BufReader::new(output);
            while let Ok(request) = incoming.recv() {
                let result = (|| -> Result<serde_json::Value> {
                    serde_json::to_writer(&mut input, &request.value)?;
                    input.write_all(b"\n")?;
                    input.flush()?;
                    let mut line = String::new();
                    anyhow::ensure!(
                        output.read_line(&mut line)? > 0,
                        "media bridge closed stdout without a response"
                    );
                    serde_json::from_str(&line)
                        .context("transcription bridge returned invalid JSON")
                })()
                .map_err(|error| format!("{error:#}"));
                let failed = result.is_err();
                let _ = request.reply.send(result);
                if failed {
                    break;
                }
            }
        });
    if let Err(error) = worker {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(job, 1);
            windows_sys::Win32::Foundation::CloseHandle(job);
        }
        #[cfg(target_os = "linux")]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).context("starting media bridge I/O worker");
    }
    Ok(MediaBridge {
        child,
        requests,
        #[cfg(windows)]
        job,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let store = ConfigStore::new(args.config.unwrap_or_else(config_path));
    privacy::set_content_logging(
        store
            .load()
            .map(|config| config.privacy.log_content)
            .unwrap_or(false),
    );
    tracing_subscriber::fmt()
        .event_format(privacy::LogFormat::default())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    match args.command {
        Mode::Daemon { no_desktop, .. } => {
            let app = App::new(store);
            serve_daemon(app, no_desktop)
        }
        Mode::Server => run_server(store),
    }
}

fn query_keyboard_language() -> Result<String> {
    let output = Command::new(sibling("simple-stt-ctl")?)
        .arg("keyboard-language")
        .output()?;
    let text = String::from_utf8(output.stdout)?;
    let language = text
        .lines()
        .find_map(|line| line.strip_prefix("message\t"))
        .context("Keyboard language unavailable")?;
    anyhow::ensure!(
        text.starts_with("status\tok"),
        "Keyboard language unavailable"
    );
    Ok(language.to_owned())
}
fn copy_ocr_text(text: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    let mut command = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut c = Command::new("wl-copy");
        c.args(["--type", "text/plain;charset=utf-8"]);
        c
    } else {
        let mut c = Command::new("xclip");
        c.args(["-selection", "clipboard", "-in"]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile","-NonInteractive","-STA","-Command","[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); $text = [Console]::In.ReadToEnd(); Set-Clipboard -Value $text"]);
        c
    };
    #[cfg(not(any(target_os = "linux", windows)))]
    anyhow::bail!("Clipboard copying is unsupported");
    #[cfg(any(target_os = "linux", windows))]
    {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child
            .stdin
            .take()
            .context("Clipboard input unavailable")?
            .write_all(text.as_bytes())?;
        let start = Instant::now();
        while child.try_wait()?.is_none() {
            if start.elapsed() > Duration::from_secs(3) {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Clipboard copy timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        anyhow::ensure!(child.wait()?.success(), "Clipboard copy failed");
        Ok(())
    }
}

#[cfg(test)]
mod server_model_tests {
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn aborted_ocr_copy_cannot_publish_late_text() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        };
        let gate = Mutex::new(());
        let epoch = AtomicU64::new(1);
        let mut clipboard = String::from("original");
        {
            let _abort = gate.lock().unwrap();
            epoch.fetch_add(1, Ordering::AcqRel);
        }
        assert!(super::commit_current_ocr(
            &gate,
            || epoch.load(Ordering::Acquire) == 1,
            || {
                clipboard = "stale OCR".into();
                Ok(())
            }
        )
        .is_err());
        assert_eq!(clipboard, "original");
        super::commit_current_ocr(
            &gate,
            || epoch.load(Ordering::Acquire) == 2,
            || {
                clipboard = "new OCR".into();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(clipboard, "new OCR");
    }
    #[test]
    fn selected_image_invalidates_queued_prewarm_without_discarding_ready_cache() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc, Mutex,
        };
        let intent = Arc::new(Mutex::new(()));
        let epoch = Arc::new(AtomicU64::new(1));
        let cache = Arc::new(Mutex::new(None));
        let (queued_tx, queued_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel();
        let gate = Arc::clone(&intent);
        let phase = Arc::clone(&epoch);
        let slot = Arc::clone(&cache);
        let late = std::thread::spawn(move || {
            queued_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            super::commit_current_ocr(
                &gate,
                || phase.load(Ordering::Acquire) == 1,
                || {
                    *slot.lock().unwrap() = Some("late worker");
                    Ok(())
                },
            )
        });
        queued_rx.recv().unwrap();
        super::end_ocr_prewarm_phase(&intent, &epoch, || true).unwrap();
        go_tx.send(()).unwrap();
        assert!(late.join().unwrap().is_err());
        assert_eq!(*cache.lock().unwrap(), None);

        // A completed prewarm stays available for recognition.
        *cache.lock().unwrap() = Some("ready worker");
        super::end_ocr_prewarm_phase(&intent, &epoch, || true).unwrap();
        assert_eq!(*cache.lock().unwrap(), Some("ready worker"));
        let before = epoch.load(Ordering::Acquire);
        assert!(super::end_ocr_prewarm_phase(&intent, &epoch, || false).is_err());
        assert_eq!(epoch.load(Ordering::Acquire), before);
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn copy_prewarm_cleanup_only_arms_its_own_published_worker() {
        assert!(!super::copy_prewarm_should_arm(0, 0));
        assert!(!super::copy_prewarm_should_arm(0, 7));
        assert!(super::copy_prewarm_should_arm(7, 7));
        assert!(!super::copy_prewarm_should_arm(7, 8));
    }

    #[test]
    fn ocr_prewarm_preserves_until_capture_and_completed_jobs_use_idle_timeout() {
        use super::{ocr_cache_decision, OcrCacheDecision};
        assert_eq!(
            ocr_cache_decision(true),
            OcrCacheDecision::PreserveForCapture
        );
        assert_eq!(ocr_cache_decision(false), OcrCacheDecision::RetainUntilIdle);
    }

    #[test]
    fn aborted_ocr_prewarm_cannot_publish_after_abort_cleanup() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        };
        let gate = Arc::new(Mutex::new(()));
        let current = Arc::new(AtomicBool::new(true));
        let cache = Arc::new(Mutex::new(None));
        let lock = gate.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let g = Arc::clone(&gate);
        let c = Arc::clone(&current);
        let slot = Arc::clone(&cache);
        let publication = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            super::commit_current_ocr(
                &g,
                || c.load(Ordering::SeqCst),
                || {
                    *slot.lock().unwrap() = Some("worker");
                    Ok(())
                },
            )
        });
        started_rx.recv().unwrap();
        current.store(false, Ordering::SeqCst);
        *cache.lock().unwrap() = None;
        drop(lock);
        assert!(publication.join().unwrap().is_err());
        assert_eq!(*cache.lock().unwrap(), None);
    }

    #[test]
    fn abort_after_ocr_publication_can_clear_the_published_worker() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        };
        let gate = Arc::new(Mutex::new(()));
        let current = Arc::new(AtomicBool::new(true));
        let cache = Arc::new(Mutex::new(None));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let g = Arc::clone(&gate);
        let c = Arc::clone(&current);
        let slot = Arc::clone(&cache);
        let publication = std::thread::spawn(move || {
            super::commit_current_ocr(
                &g,
                || c.load(Ordering::SeqCst),
                || {
                    *slot.lock().unwrap() = Some("worker");
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )
        });
        entered_rx.recv().unwrap();
        let g = Arc::clone(&gate);
        let c = Arc::clone(&current);
        let slot = Arc::clone(&cache);
        let abort = std::thread::spawn(move || {
            let _guard = g.lock().unwrap();
            c.store(false, Ordering::SeqCst);
            *slot.lock().unwrap() = None;
        });
        release_tx.send(()).unwrap();
        publication.join().unwrap().unwrap();
        abort.join().unwrap();
        assert_eq!(*cache.lock().unwrap(), None);
    }

    #[test]
    #[ignore = "run scripts/test-read-along-remote.py with built helper binaries and a private profile"]
    fn remote_read_along_chunks_overlap_independent_ocr() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc, Mutex,
        };
        use std::time::Duration;
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let base = format!("http://{}", server.server_addr());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured = observed.clone();
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let serving = std::thread::spawn(move || {
            let mut children = Vec::new();
            let mut speech = 0;
            for _ in 0..3 {
                let mut request = server
                    .recv_timeout(Duration::from_secs(15))
                    .unwrap()
                    .expect("client request missing");
                let path = request.url().to_owned();
                let body: serde_json::Value = serde_json::from_reader(request.as_reader()).unwrap();
                let is_speech = path.contains("audio/speech");
                if is_speech {
                    speech += 1;
                }
                let block = is_speech && speech == 2;
                captured.lock().unwrap().push((path, body));
                let blocked_tx = blocked_tx.clone();
                let release_rx = release_rx.clone();
                children.push(std::thread::spawn(move || {
                    if block { blocked_tx.send(()).unwrap(); release_rx.lock().unwrap().recv_timeout(Duration::from_secs(15)).unwrap(); }
                    if is_speech {
                        request.respond(tiny_http::Response::from_data(vec![0;4800]).with_header(tiny_http::Header::from_bytes("Content-Type", "audio/pcm;rate=24000;channels=1").unwrap())).unwrap();
                    } else {
                        request.respond(super::server_json(200, &serde_json::json!({"choices":[{"message":{"content":"Next page OCR completed during speech synthesis."}}]}))).unwrap();
                    }
                }));
            }
            for child in children {
                child.join().unwrap();
            }
        });
        let mut config = super::AppConfig::default();
        config.speech.selection.mode = reader_core::routing::SelectionMode::SingleModel;
        config.speech.provider = reader_core::SpeechProvider::OpenAiCompatible {
            base_url: format!("{base}/tts/v1"),
            model: "remote-tts".into(),
            voice: "test-voice".into(),
            format: "pcm".into(),
        };
        config.speech.use_feature_key = false;
        config.speech.fallback_to_local = false;
        config.speech.excluded_words = vec!["SKIPME".into()];
        config.ocr.provider = reader_core::OcrProvider::OpenAiCompatible {
            base_url: format!("{base}/ocr/v1"),
            model: "remote-ocr".into(),
            api: reader_core::config::OcrApi::ChatCompletions,
        };
        config.ocr.fallback_to_local = false;
        let path = reader_core::config::config_path();
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let app = super::App::new(reader_core::ConfigStore::new(path));
        app.inner.desktop_enabled.store(false, Ordering::Release);
        let units =
            reader_core::text::plan_units(&format!("{}.", "SKIPME ordinaryword ".repeat(80)), 240)
                .unwrap();
        assert!(units.len() > 2);
        let first = app
            .prepare_read_along_chunk(&config, &units[0], 0, false, &|| true)
            .unwrap();
        assert_eq!(first.samples.len(), 2400);
        let producer = app.clone();
        let unit = units[1].clone();
        let done = Arc::new(AtomicBool::new(false));
        let finished = done.clone();
        let producer_config = config.clone();
        let second = std::thread::spawn(move || {
            let result =
                producer.prepare_read_along_chunk(&producer_config, &unit, 0, true, &|| true);
            finished.store(true, Ordering::Release);
            result
        });
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let image = reader_core::config::data_dir().join("capture/mock.png");
        let started = std::time::Instant::now();
        let text = super::run_ocr(&app, &config, &image, 20_000_000, false, || true, None).unwrap();
        assert!(text.contains("Next page OCR"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            !done.load(Ordering::Acquire),
            "OCR must complete while TTS is still blocked"
        );
        release_tx.send(()).unwrap();
        let next = second.join().unwrap().unwrap();
        assert_eq!(next.samples.len(), 2400);
        assert_eq!(next.unit.unit_index, units[1].unit_index);
        serving.join().unwrap();
        let requests = observed.lock().unwrap();
        let speech: Vec<_> = requests
            .iter()
            .filter(|(path, _)| path.contains("audio/speech"))
            .collect();
        assert_eq!(speech.len(), 2);
        for (_, body) in speech {
            let input = body["input"].as_str().unwrap();
            assert!(input.len() <= 240);
            assert!(!input.contains("SKIPME"));
            assert_eq!(body["model"], "remote-tts");
        }
        app.action_unload().unwrap();
    }

    #[test]
    fn failed_new_read_does_not_publish_the_previous_session_as_live() {
        let directory = tempfile::tempdir().unwrap();
        let app = super::App::new(reader_core::ConfigStore::new(
            directory.path().join("config.json"),
        ));
        app.inner
            .desktop_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        let old = app.inner.generation.advance();
        *app.inner.session.lock().unwrap() = Some(super::Session {
            job_id: "old".into(),
            generation: old,
            source: "read-along".into(),
            units: reader_core::text::plan_units("Old page.", 240).unwrap(),
            preparation: std::sync::Arc::new(super::chunk_stream::Progress::new(1)),
        });
        let current = app.inner.generation.advance();
        app.inner.activity.lock().unwrap().begin(current);
        app.report_intent_failure(
            current,
            "Read-along OCR failed",
            &anyhow::anyhow!("provider returned HTTP 400"),
            false,
        );
        let status = app.status();
        assert_eq!(status["state"], "idle");
        assert_eq!(status["chunk_total"], 0);
        assert_eq!(status["chunk_text"], "");
        app.inner.notice.lock().unwrap().expires = Some(std::time::Instant::now());
        assert_eq!(app.status()["notice_text"], "");
        assert!(!app.failed_read_along_area_is_saved());
        let revision = app.begin_repeat_action("read-along");
        assert!(!app.failed_read_along_area_is_saved());
        app.complete_repeat_action(
            revision,
            super::repeat::RepeatPayload::ReadAlong {
                target: super::read_along_capture::Target {
                    window: super::read_along_capture::WindowId::X11(77),
                    region: super::read_along_capture::Rect {
                        x: 2,
                        y: 3,
                        width: 400,
                        height: 200,
                    },
                },
            },
        );
        assert!(app.failed_read_along_area_is_saved());
        app.inner.activity.lock().unwrap().set(current, "idle");
        assert!(!app.failed_read_along_area_is_saved());
    }
    #[test]
    fn read_along_hotkey_stops_an_active_run_without_selecting_again() {
        let directory = tempfile::tempdir().unwrap();
        let app = super::App::new(reader_core::ConfigStore::new(
            directory.path().join("config.json"),
        ));
        app.inner
            .desktop_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        app.inner
            .read_along_active
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(app.action_read_along().unwrap().contains("stopped"));
        assert!(!app
            .inner
            .read_along_active
            .load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn exclusions_apply_to_remote_speech_without_mutating_source_text() {
        let mut config = super::AppConfig::default();
        config.speech.provider = reader_core::SpeechProvider::OpenAiCompatible {
            base_url: "https://example.com/v1".into(),
            model: "tts".into(),
            voice: "voice".into(),
            format: "wav".into(),
        };
        config.speech.excluded_words = vec!["marker".into()];
        let original = "Read MARKER this.".to_owned();
        assert_eq!(
            super::prepare_speech_text(&config, &original, false),
            "Read  this."
        );
        assert_eq!(original, "Read MARKER this.");
        assert!(super::prepare_speech_text(&config, "marker", false).is_empty());
    }

    #[test]
    fn prepared_speech_reaches_the_wire_after_model_selection() {
        let mut config = super::AppConfig::default();
        config.speech.selection.single.model = Some("kokoro".into());
        config.speech.selection.single.lowercase = true;
        let text = super::prepare_speech_text(&config, "NASA 42!", false);
        let request = reader_core::wire::WorkerRequest::Synthesize {
            job_id: "job".into(),
            generation: 1,
            sentence: 0,
            unit: 0,
            text,
            voice: "af_heart".into(),
            steps: None,
            language: "en".into(),
            speed: 1.0,
            device: "cpu".into(),
        };
        assert_eq!(serde_json::to_value(request).unwrap()["text"], "nasa 42!");
        config.speech.provider = reader_core::SpeechProvider::LocalSupertonic;
        assert_eq!(
            super::prepare_speech_text(&config, "NASA 42!", false),
            "NASA 42!"
        );
        config.speech.language = "ar".into();
        config.speech.selection.single.model = Some("supertonic3".into());
        let text = super::prepare_speech_text(&config, "٥$ ۱۲۳", false);
        let request = reader_core::wire::WorkerRequest::Synthesize {
            job_id: "job".into(),
            generation: 1,
            sentence: 0,
            unit: 0,
            text,
            voice: "sid:0".into(),
            steps: None,
            language: "ar".into(),
            speed: 1.0,
            device: "cpu".into(),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap()["text"],
            "خمسة دولارات 123"
        );
    }
    #[test]
    fn repeated_page_fingerprint_ignores_spacing_and_ocr_case_jitter() {
        assert_eq!(
            super::text_fingerprint("Dialogue BOX\n Next line"),
            super::text_fingerprint("dialogue box next LINE")
        );
        assert_ne!(
            super::text_fingerprint("First page"),
            super::text_fingerprint("Second page")
        );
    }

    #[test]
    fn generation_options_do_not_change_worker_fingerprint() {
        let mut config = super::AppConfig::default();
        let original = super::fingerprint(&config);
        config.speech.voice = "sid:3".into();
        config.speech.selection.single.voice = Some("sid:3".into());
        config.speech.selection.single.steps = Some(9);
        config.speech.selection.single.speed_percent = Some(130);
        config.speech.speed = 1.3;
        assert_eq!(super::fingerprint(&config), original);
    }
    #[test]
    fn clipboard_privacy_hints_are_exact_and_value_sensitive() {
        use super::{
            clipboard_dword_disallows, clipboard_has_format, clipboard_kde_hint_is_secret,
        };
        assert!(clipboard_has_format(
            "text/plain\nX-KDE-PASSWORDMANAGERHINT\n",
            "x-kde-passwordManagerHint"
        ));
        assert!(!clipboard_has_format(
            "x-kde-passwordManagerHint-extra\n",
            "x-kde-passwordManagerHint"
        ));
        assert!(clipboard_kde_hint_is_secret(b"secret"));
        assert!(clipboard_kde_hint_is_secret(b" SECRET\n"));
        assert!(!clipboard_kde_hint_is_secret(b"none"));
        assert!(!clipboard_kde_hint_is_secret(b"secret-extra"));
        assert!(clipboard_dword_disallows(&[0, 0, 0, 0]));
        assert!(!clipboard_dword_disallows(&[1, 0, 0, 0]));
        assert!(!clipboard_dword_disallows(&[0, 0, 0]));
    }
    use super::{next_job_event_sequence, requested_server_model, wait_until_modifiers_released};
    use std::time::Duration;

    #[cfg(target_os = "linux")]
    fn fake_media_bridge(script: &str) -> super::MediaBridge {
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        super::start_media_bridge_command(command).unwrap()
    }

    fn test_lifetime(
        total: Duration,
        queue: Duration,
        cold_load: Duration,
    ) -> super::request_lifetime::RequestLifetime {
        let budgets = reader_core::request_budget::RequestBudgets {
            total,
            queue,
            cold_model_load: cold_load,
            ..Default::default()
        };
        super::request_lifetime::RequestLifetime::new("test", true, budgets)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stalled_media_bridge_is_killed_and_next_request_reconnects() {
        let bridge = fake_media_bridge("IFS= read -r request; sleep 30");
        let pid = bridge.child.id();
        let lifetime = test_lifetime(
            Duration::from_secs(2),
            Duration::from_millis(100),
            Duration::from_millis(40),
        );
        lifetime.set_stage(reader_core::request_budget::RequestStage::LoadingModel);
        assert!(bridge
            .transcribe(serde_json::json!({"path":"unused"}), &lifetime)
            .is_err());
        drop(bridge);
        for _ in 0..50 {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());

        let bridge = fake_media_bridge(
            "while IFS= read -r request; do printf '{\"text\":\"ok\",\"pid\":%s}\\n' \"$$\"; done",
        );
        let first = test_lifetime(
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let second = test_lifetime(
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let a = bridge
            .transcribe(serde_json::json!({"path":"one"}), &first)
            .unwrap();
        let b = bridge
            .transcribe(serde_json::json!({"path":"two"}), &second)
            .unwrap();
        assert_eq!(a["text"], "ok");
        assert_eq!(
            a["pid"], b["pid"],
            "the bridge remains warm across requests"
        );
    }

    #[test]
    fn media_bridge_queue_wait_observes_request_deadline() {
        let mutex = std::sync::Mutex::new(None);
        let _held = mutex.lock().unwrap();
        let lifetime = test_lifetime(
            Duration::from_secs(1),
            Duration::from_millis(30),
            Duration::from_secs(1),
        );
        let started = std::time::Instant::now();
        assert!(super::acquire_media_bridge(&mutex, &lifetime).is_err());
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn screenshot_settling_begins_after_the_recorded_action_finishes() {
        use reader_core::recorded_action::{KeyEvent, Playback, RecordedAction};
        struct TimedPlayback {
            start: std::time::Instant,
            released: Option<std::time::Instant>,
        }
        impl Playback for TimedPlayback {
            fn wait_until(&mut self, at_ms: u32) -> anyhow::Result<()> {
                std::thread::sleep(
                    std::time::Duration::from_millis(u64::from(at_ms))
                        .saturating_sub(self.start.elapsed()),
                );
                Ok(())
            }
            fn key(&mut self, _: &str, down: bool) -> anyhow::Result<()> {
                if !down {
                    self.released = Some(std::time::Instant::now());
                }
                Ok(())
            }
        }
        let action = RecordedAction {
            events: vec![
                KeyEvent {
                    key: "Space".into(),
                    down: true,
                    at_ms: 0,
                },
                KeyEvent {
                    key: "Space".into(),
                    down: false,
                    at_ms: 20,
                },
                KeyEvent {
                    key: "Space".into(),
                    down: true,
                    at_ms: 40,
                },
                KeyEvent {
                    key: "Space".into(),
                    down: false,
                    at_ms: 60,
                },
            ],
        };
        let mut playback = TimedPlayback {
            start: std::time::Instant::now(),
            released: None,
        };
        reader_core::recorded_action::replay(&action, &mut playback).unwrap();
        let advanced = std::time::Instant::now();
        super::wait_read_along_settle(advanced, 30, &|| true).unwrap();
        let screenshot = std::time::Instant::now();
        assert!(
            screenshot.duration_since(playback.released.unwrap())
                >= std::time::Duration::from_millis(30)
        );
        assert!(screenshot.duration_since(playback.start) >= std::time::Duration::from_millis(90));
    }

    #[test]
    fn failed_next_page_finishes_and_notice_expires() {
        let directory = tempfile::tempdir().unwrap();
        let app = super::App::new(reader_core::ConfigStore::new(
            directory.path().join("config.json"),
        ));
        app.inner
            .desktop_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        let app_generation = app.inner.generation.advance();
        let mut controller = super::read_along::Controller::new(1);
        let generation = controller.generation();
        controller.set_current_page(generation, 10);
        controller.request_prefetch(generation);
        controller.audio_finished(generation);
        let mut preparation = super::read_along::Preparation::new(true);
        preparation.text = Some("pending speech text".into());
        *app.inner.read_along.lock().unwrap() = Some(super::ReadAlongRun {
            controller,
            target: super::read_along_capture::Target {
                window: super::read_along_capture::WindowId::X11(1),
                region: super::read_along_capture::Rect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            },
            app_generation,
            settings: Default::default(),
            prepared: None,
            next_progress: None,
            next_activity: Some(super::ReadAlongNextActivity {
                phase: "capturing",
                waiting_until: None,
            }),
            first_advance: None,
            preparation,
            active_stream: None,
        });
        app.read_along_failed(
            generation,
            app_generation,
            anyhow::anyhow!("mock transport failure"),
        );
        assert!(app.inner.read_along.lock().unwrap().is_none());
        assert_eq!(app.status()["read_along_active"], false);
        assert!(app
            .inner
            .notice
            .lock()
            .unwrap()
            .text
            .contains("Retry last action"));
        app.inner.notice.lock().unwrap().expires = Some(std::time::Instant::now());
        assert_eq!(app.status()["notice_text"], "");
        assert_eq!(app.status()["state"], "idle");
    }

    #[test]
    fn read_along_failure_clears_pending_and_keeps_generation_actionable() {
        let directory = tempfile::tempdir().unwrap();
        let app = super::App::new(reader_core::ConfigStore::new(
            directory.path().join("reader-config.json"),
        ));
        app.inner
            .desktop_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        let generation = app.inner.generation.advance();
        app.inner.activity.lock().unwrap().begin(generation);
        app.inner
            .read_pending
            .store(true, std::sync::atomic::Ordering::Release);
        app.report_intent_failure(
            generation,
            "Read-along selection failed",
            &anyhow::anyhow!("window query was refused"),
            false,
        );
        assert!(!app
            .inner
            .read_pending
            .load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(app.inner.activity.lock().unwrap().phase, "error");
        assert!(app
            .inner
            .notice
            .lock()
            .unwrap()
            .text
            .contains("window query was refused"));
    }

    #[test]
    fn unused_playback_does_not_prevent_render_only_worker_expiry() {
        let audio = super::AudioOutput::new();
        assert!(super::audio_is_idle(&audio));
        audio.inner.state.lock().unwrap().producer_done = false;
        assert!(
            !super::audio_is_idle(&audio),
            "an active producer must retain its worker"
        );
    }

    #[test]
    fn selected_monolingual_model_does_not_redetect_each_remote_chunk() {
        let languages = vec!["en".to_string()];
        let text = "Questa è una frase italiana molto chiara.";
        assert_eq!(reader_core::language::resolve(text), "it");
        assert_eq!(
            super::server_speech_language(
                &languages,
                &serde_json::json!({"model":"kokoro-v1"}),
                text
            ),
            "en"
        );
        assert_eq!(
            super::server_speech_language(&languages, &serde_json::json!({"language":"ar"}), text),
            "ar"
        );
    }
    #[test]
    fn model_validation_defaults_and_rejects_unknown_ids() {
        let request = serde_json::json!({});
        assert_eq!(
            requested_server_model(&request, "kokoro-v1", &["kokoro-v1"]).unwrap(),
            "kokoro-v1"
        );
        let request = serde_json::json!({"model":"simple-tts-ocr"});
        assert_eq!(
            requested_server_model(
                &request,
                "simple-tts-ocr",
                &["simple-tts-ocr", "paddle-ar-en"]
            )
            .unwrap(),
            "simple-tts-ocr"
        );
        let request = serde_json::json!({"model":"not-installed"});
        assert!(requested_server_model(&request, "kokoro-v1", &["kokoro-v1"]).is_err());
        let request = serde_json::json!({"model":17});
        assert!(requested_server_model(&request, "kokoro-v1", &["kokoro-v1"]).is_err());
    }

    #[test]
    fn pruned_job_events_keep_a_monotonic_sequence() {
        let mut events = Vec::new();
        for _ in 0..300 {
            let sequence = next_job_event_sequence(&events);
            events.push(serde_json::json!({"sequence": sequence}));
            if events.len() > 128 {
                events.remove(0);
            }
        }
        assert_eq!(events.len(), 128);
        let sequences = events
            .iter()
            .map(|event| event["sequence"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(sequences.last().copied(), Some(299));
    }

    #[test]
    fn modifier_wait_is_cancellable_and_requires_a_stable_release() {
        assert!(
            wait_until_modifiers_released(|| false, || Ok(false), Duration::from_secs(2)).is_err()
        );

        let mut states = [true, false, false, false].into_iter();
        let result = wait_until_modifiers_released(
            || true,
            || Ok(states.next().unwrap_or(false)),
            Duration::from_secs(1),
        );
        assert!(result.is_ok());
    }
}
