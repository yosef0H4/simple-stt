//! A temporary Wayland selection with a complete, bounded backup. Restoration
//! follows completed payload transfers, never an unconditional post-key timer.
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
    ext_data_control_offer_v1 as offer, ext_data_control_source_v1 as source,
};

const MAX_FORMAT: usize = 32 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
// Qt image offers can serialize expensive conversions for many MIME aliases.
// Bound total work, but allow ongoing transfers to finish rather than imposing
// one five-second deadline shared by every format.
pub const BACKUP_LIMIT: Duration = Duration::from_secs(45);
const BACKUP_STALL_LIMIT: Duration = Duration::from_secs(8);
const SETTLE: Duration = Duration::from_millis(250);
const READ_LIMIT: Duration = Duration::from_secs(8);
type PayloadMap = HashMap<String, Arc<Vec<u8>>>;
type Payloads = HashMap<(u64, bool), PayloadMap>;

/// Metadata exposed by the private resident clipboard cache socket. Payloads
/// are never serialized by this status endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
pub struct CacheStatus {
    #[serde(default)]
    pub instance: u32,
    pub generation: u64,
    pub ready: bool,
    pub warming: bool,
    pub formats: usize,
    #[serde(rename = "bytes")]
    pub cached_bytes: usize,
    #[serde(default)]
    pub events: Vec<CacheEvent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CacheEvent {
    pub sequence: u64,
    pub generation: u64,
    pub phase: CachePhase,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CachePhase {
    Buffering,
    Ready,
    Cancelled,
    Failed,
}

fn publish_status(status: &std::sync::Mutex<CacheStatus>, mut next: CacheStatus) {
    let mut current = status.lock().unwrap();
    next.instance = current.instance;
    next.events = std::mem::take(&mut current.events);
    *current = next;
}

fn publish_event(status: &std::sync::Mutex<CacheStatus>, generation: u64, phase: CachePhase) {
    let mut status = status.lock().unwrap();
    let sequence = status.events.last().map_or(1, |event| event.sequence + 1);
    if status.events.len() == 32 {
        status.events.remove(0);
    }
    status.events.push(CacheEvent {
        sequence,
        generation,
        phase,
    });
    eprintln!("[simple-stt-linux] clipboard buffer generation={generation} phase={phase:?} formats={} bytes={}", status.formats, status.cached_bytes);
}

enum CacheRequest {
    Transaction(UnixStream, bool, String),
    Enable,
    Disable,
}

/// Run the current clipboard watcher on its own Wayland connection. The
/// watcher retains only the latest complete selection and replaces it as soon
/// as the compositor reports a different offer. Its private socket carries
/// metadata queries and bounded transaction text; clipboard payloads stay here.
pub fn serve_cache_daemon(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            bail!("clipboard cache daemon is already running");
        }
        std::fs::remove_file(socket_path).context("removing stale clipboard cache socket")?;
    }
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("binding clipboard cache socket {}", socket_path.display()))?;
    let _socket_cleanup = SocketCleanup(socket_path.to_path_buf());
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
    let published_status = Arc::new(std::sync::Mutex::new(CacheStatus {
        instance: std::process::id(),
        generation: 0,
        ready: false,
        warming: true,
        formats: 0,
        cached_bytes: 0,
        ..CacheStatus::default()
    }));
    let status_server = Arc::clone(&published_status);
    let (transaction_sender, transaction_receiver) = mpsc::channel();
    std::thread::spawn(move || loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if !is_same_user(&stream).unwrap_or(false) {
                    continue;
                }
                let status = status_server.lock().unwrap().clone();
                let sender = transaction_sender.clone();
                std::thread::spawn(move || {
                    if let Err(error) = handle_cache_request(stream, &status, &sender) {
                        eprintln!("[simple-stt-linux] clipboard cache request failed: {error:#}");
                    }
                });
            }
            Err(error) => {
                eprintln!("[simple-stt-linux] clipboard cache socket stopped: {error}");
                break;
            }
        }
    });

    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let handle = queue.handle();
    let manager: manager::ExtDataControlManagerV1 = globals.bind(&handle, 1..=1, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=9, ())?;
    let device = manager.get_data_device(&seat, &handle, ());
    let (sender, receiver) = mpsc::channel();
    let mut state = State {
        transfers: Some(sender),
        ..State::default()
    };
    queue.roundtrip(&mut state)?;

    let mut generation = 0u64;
    let mut cached_offer = None;
    let mut cache = HashMap::<String, Arc<Vec<u8>>>::new();
    let mut cache_ready = false;
    let mut enabled = true;
    loop {
        pump(&mut queue, &mut state, &connection)?;
        let selected = state.clipboard.clone();
        if enabled && state.restored_sources.is_empty() && selected != cached_offer {
            // Drop the old generation before reading the replacement. A
            // failed/slow offer can never leave stale bytes eligible for use.
            generation = generation.wrapping_add(1);
            cached_offer = selected.clone();
            cache.clear();
            publish_status(
                &published_status,
                CacheStatus {
                    generation,
                    ready: false,
                    warming: selected.is_some(),
                    formats: 0,
                    cached_bytes: 0,
                    ..CacheStatus::default()
                },
            );
            if selected.is_some() {
                publish_event(&published_status, generation, CachePhase::Buffering);
            }
            cache_ready = if let Some(selected) = selected.as_ref() {
                match backup(&mut queue, &mut state, &connection, Some(selected)) {
                    Ok(payloads)
                        if state.clipboard.as_ref() == Some(selected)
                            && state.primary.as_ref() != Some(selected) =>
                    {
                        cache = payloads;
                        true
                    }
                    Ok(_) => {
                        cache.clear();
                        cached_offer = None;
                        false
                    }
                    Err(error) => {
                        eprintln!(
                            "[simple-stt-linux] current clipboard cache unavailable: {error:#}"
                        );
                        cache.clear();
                        false
                    }
                }
            } else {
                true
            };
            publish_status(
                &published_status,
                CacheStatus {
                    generation,
                    ready: cache_ready,
                    warming: false,
                    formats: cache.len(),
                    cached_bytes: cache.values().map(|bytes| bytes.len()).sum(),
                    ..CacheStatus::default()
                },
            );
            if selected.is_some() {
                let phase = if state.clipboard != selected {
                    CachePhase::Cancelled
                } else if cache_ready {
                    CachePhase::Ready
                } else {
                    CachePhase::Failed
                };
                publish_event(&published_status, generation, phase);
            }
        }
        // If a new selection arrived while its formats were being read, leave
        // queued transactions for the next iteration so only the latest offer
        // can be considered for reuse.
        if state.clipboard == selected {
            while let Ok(request) = transaction_receiver.try_recv() {
                match request {
                    CacheRequest::Enable => {
                        enabled = true;
                        if let Some(epoch) = state.restored_sources.keys().next().copied() {
                            cache = state
                                .payloads
                                .get(&(epoch, false))
                                .cloned()
                                .unwrap_or_default();
                            cache_ready = true;
                            publish_status(
                                &published_status,
                                CacheStatus {
                                    generation,
                                    ready: true,
                                    warming: false,
                                    formats: cache.len(),
                                    cached_bytes: cache.values().map(|bytes| bytes.len()).sum(),
                                    ..CacheStatus::default()
                                },
                            );
                        }
                    }
                    CacheRequest::Disable => {
                        enabled = false;
                        cache.clear();
                        cache_ready = false;
                        cached_offer = None;
                        publish_status(
                            &published_status,
                            CacheStatus {
                                generation,
                                ready: false,
                                warming: false,
                                formats: 0,
                                cached_bytes: 0,
                                ..CacheStatus::default()
                            },
                        );
                    }
                    CacheRequest::Transaction(stream, primary, text) => {
                        if enabled {
                            let mut owner = WaylandOwner {
                                queue: &mut queue,
                                state: &mut state,
                                connection: &connection,
                                handle: &handle,
                                manager: &manager,
                                device: &device,
                                receiver: &receiver,
                            };
                            if let Err(error) = daemon_transaction(
                                stream,
                                primary,
                                text,
                                &mut owner,
                                cached_offer.as_ref(),
                                &cache,
                                cache_ready,
                            ) {
                                eprintln!("[simple-stt-linux] cached clipboard transaction failed: {error:#}");
                            }
                        }
                        // A transaction may have changed clipboard ownership.
                        // Repump and invalidate the cache before serving another
                        // queued client against the previous offer generation.
                        break;
                    }
                }
            }
        }
        if !enabled && state.restored_sources.is_empty() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn handle_cache_request(
    stream: UnixStream,
    status: &CacheStatus,
    transactions: &mpsc::Sender<CacheRequest>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let line = read_bounded_line(&mut reader, MAX_FORMAT + 256)?;
    if line == b"status\n" {
        return serve_cache_status(stream, status);
    }
    if line == b"disable\n" || line == b"enable\n" {
        let request = if line == b"disable\n" {
            CacheRequest::Disable
        } else {
            CacheRequest::Enable
        };
        transactions
            .send(request)
            .map_err(|_| anyhow::anyhow!("clipboard cache service stopped"))?;
        return Ok(());
    }
    #[derive(serde::Deserialize)]
    struct Request {
        primary: bool,
        text: String,
    }
    let request: Request = serde_json::from_slice(&line)?;
    if request.text.is_empty() || request.text.len() > MAX_FORMAT {
        bail!("invalid clipboard transaction text");
    }
    stream.set_read_timeout(None)?;
    transactions
        .send(CacheRequest::Transaction(
            stream,
            request.primary,
            request.text,
        ))
        .map_err(|_| anyhow::anyhow!("clipboard transaction service stopped"))
}

fn read_bounded_line(reader: &mut impl BufRead, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            bail!("clipboard cache request ended before newline");
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |i| i + 1);
        if bytes.len().saturating_add(count) > limit {
            bail!("clipboard cache request exceeds its size limit");
        }
        let done = available[count - 1] == b'\n';
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if done {
            return Ok(bytes);
        }
    }
}

struct SocketCleanup(std::path::PathBuf);

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn is_same_user(stream: &UnixStream) -> Result<bool> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error())
            .context("reading clipboard cache peer credentials");
    }
    Ok(credentials.uid == unsafe { libc::getuid() })
}

fn serve_cache_status(mut stream: UnixStream, status: &CacheStatus) -> Result<()> {
    serde_json::to_writer(&mut stream, status)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

pub fn query_cache_status(socket_path: &Path) -> Result<CacheStatus> {
    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to clipboard cache {}", socket_path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    writeln!(stream, "status")?;
    stream.flush()?;
    let mut response = String::new();
    std::io::BufReader::new(stream).read_line(&mut response)?;
    if response.len() > 16384 {
        bail!("clipboard cache status response is too large");
    }
    Ok(serde_json::from_str(&response)?)
}

pub fn set_cache_enabled(socket_path: &Path, enabled: bool) -> Result<bool> {
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(stream) => stream,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.kind() == std::io::ErrorKind::ConnectionRefused =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    writeln!(stream, "{}", if enabled { "enable" } else { "disable" })?;
    stream.flush()?;
    Ok(true)
}

/// Proxy the existing child stdio protocol to the resident Wayland owner.
/// `false` means the socket was unavailable before any request was submitted,
/// so the caller may safely use the legacy cold path.
pub fn proxy_stdio(primary: bool, socket_path: &Path) -> Result<bool> {
    let mut socket = match UnixStream::connect(socket_path) {
        Ok(socket) => socket,
        Err(_) => return Ok(false),
    };
    let _shutdown = SocketShutdown(socket.try_clone()?);
    let mut input = std::io::stdin();
    let line = read_bounded_line(&mut input.lock(), MAX_FORMAT + 16)?;
    let text: String = serde_json::from_slice(&line)?;
    if text.is_empty() || text.len() > MAX_FORMAT {
        bail!("invalid clipboard transaction text");
    }
    serde_json::to_writer(
        &mut socket,
        &serde_json::json!({ "primary": primary, "text": text }),
    )?;
    socket.write_all(b"\n")?;
    socket.flush()?;
    let mut writer = socket.try_clone()?;
    let input_thread = std::thread::spawn(move || {
        let _ = std::io::copy(&mut input, &mut writer);
        let _ = writer.shutdown(std::net::Shutdown::Write);
    });
    drop(input_thread);
    let mut reader = std::io::BufReader::new(&socket);
    let mut output = std::io::stdout();
    loop {
        let response = read_bounded_line(&mut reader, 32)?;
        output.write_all(&response)?;
        output.flush()?;
        if matches!(
            response.as_slice(),
            b"restored\n" | b"unread\n" | b"replaced\n"
        ) {
            let _ = socket.shutdown(std::net::Shutdown::Write);
            return Ok(true);
        }
    }
}

struct SocketShutdown(UnixStream);

impl Drop for SocketShutdown {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

struct WaylandOwner<'a> {
    queue: &'a mut EventQueue<State>,
    state: &'a mut State,
    connection: &'a Connection,
    handle: &'a QueueHandle<State>,
    manager: &'a manager::ExtDataControlManagerV1,
    device: &'a device::ExtDataControlDeviceV1,
    receiver: &'a mpsc::Receiver<(u64, bool)>,
}

fn daemon_transaction(
    mut stream: UnixStream,
    primary: bool,
    text: String,
    owner: &mut WaylandOwner<'_>,
    cached_offer: Option<&offer::ExtDataControlOfferV1>,
    cache: &PayloadMap,
    cache_ready: bool,
) -> Result<()> {
    let WaylandOwner {
        queue,
        state,
        connection,
        handle,
        manager,
        device,
        receiver,
    } = owner;
    let original = if primary {
        state.primary.clone()
    } else {
        state.clipboard.clone()
    };
    let retained_source = state.restored_sources.keys().next().copied();
    let payloads = if !primary {
        if let Some(epoch) = retained_source {
            state
                .payloads
                .get(&(epoch, false))
                .cloned()
                .unwrap_or_else(|| cache.clone())
        } else if cache_offer_eligible(cache_ready, original.as_ref() == cached_offer) {
            cache.clone()
        } else {
            backup(queue, state, connection, original.as_ref())?
        }
    } else {
        backup(queue, state, connection, original.as_ref())?
    };
    queue.roundtrip(state)?;
    let current = if primary {
        &state.primary
    } else {
        &state.clipboard
    };
    let still_owned =
        retained_source.is_some_and(|epoch| state.restored_sources.contains_key(&epoch));
    if current != &original && !still_owned {
        bail!("clipboard changed during cache lookup; paste not started");
    }

    let epoch = state.next_epoch;
    state.next_epoch = state.next_epoch.wrapping_add(1);
    state.cancelled.remove(&(epoch, true));
    state.cancelled.remove(&(epoch, false));
    state.active.remove(&epoch);
    state.read_finished.remove(&epoch);
    state.active_transactions.insert(epoch);
    state.payloads.insert((epoch, false), payloads);
    let mut transcript_payloads = HashMap::new();
    for mime in [
        "text/plain;charset=utf-8",
        "text/plain",
        "x-kde-passwordManagerHint",
    ] {
        transcript_payloads.insert(
            mime.to_owned(),
            Arc::new(if mime.starts_with("text/") {
                text.as_bytes().to_vec()
            } else {
                b"secret".to_vec()
            }),
        );
    }
    state.payloads.insert((epoch, true), transcript_payloads);
    let result = (|| -> Result<()> {
        let temporary = manager.create_data_source(handle, (epoch, true));
        for mime in [
            "text/plain;charset=utf-8",
            "text/plain",
            "x-kde-passwordManagerHint",
        ] {
            temporary.offer(mime.to_owned());
        }
        if primary {
            device.set_primary_selection(Some(&temporary));
        } else {
            device.set_selection(Some(&temporary));
        }
        queue.roundtrip(state)?;
        if state.cancelled.contains(&(epoch, true)) {
            return reject_unpublished_transaction(state, epoch);
        }

        let mut input = stream.try_clone()?;
        let (commands, command_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(&mut input);
            while let Ok(line) = read_bounded_line(&mut reader, 16) {
                let command = String::from_utf8_lossy(&line).trim().to_owned();
                if commands.send(command).is_err() {
                    return;
                }
            }
            let _ = commands.send("abort".to_owned());
        });
        let mut failed = writeln!(stream, "ready")
            .and_then(|()| stream.flush())
            .is_err();
        let preparation_deadline = Instant::now() + Duration::from_secs(60);
        let mut sent_at = None;
        loop {
            while let Ok(command) = command_rx.try_recv() {
                match command.as_str() {
                    "arm" => {
                        state.armed = Some(epoch);
                        state.read_finished.remove(&epoch);
                        failed |= writeln!(stream, "armed")
                            .and_then(|()| stream.flush())
                            .is_err();
                    }
                    "sent" => sent_at = Some(Instant::now()),
                    _ => failed = true,
                }
            }
            pump(queue, state, connection)?;
            if state.cancelled.contains(&(epoch, true)) {
                let _ = writeln!(stream, "replaced").and_then(|()| stream.flush());
                cleanup_transaction(state, epoch, false);
                return Ok(());
            }
            while let Ok((completed_epoch, ok)) = receiver.try_recv() {
                if completed_epoch != epoch {
                    continue;
                }
                apply_transfer_result(state, epoch, completed_epoch, ok);
                if completed_epoch == epoch && !ok {
                    failed = true;
                }
            }
            let complete = sent_at.is_some_and(|at| at.elapsed() >= SETTLE)
                && state.active.get(&epoch).copied().unwrap_or(0) == 0
                && state
                    .read_finished
                    .get(&epoch)
                    .is_some_and(|at| at.elapsed() >= SETTLE);
            let timed_out = sent_at.map_or_else(
                || Instant::now() >= preparation_deadline,
                |at| at.elapsed() >= READ_LIMIT,
            );
            if failed || timed_out || complete {
                failed |= !complete;
                queue.roundtrip(state)?;
                if state.cancelled.contains(&(epoch, true)) {
                    let _ = writeln!(stream, "replaced").and_then(|()| stream.flush());
                    cleanup_transaction(state, epoch, false);
                    return Ok(());
                }
                break;
            }
        }
        let restored_payloads = state.payloads.get(&(epoch, false));
        let restored = if restored_payloads.is_none_or(HashMap::is_empty) {
            None
        } else {
            let source = manager.create_data_source(handle, (epoch, false));
            for mime in restored_payloads.unwrap().keys() {
                source.offer(mime.clone());
            }
            Some(source)
        };
        if primary {
            device.set_primary_selection(restored.as_ref());
        } else {
            device.set_selection(restored.as_ref());
        }
        queue.roundtrip(state)?;
        let _ = writeln!(stream, "{}", if failed { "unread" } else { "restored" })
            .and_then(|()| stream.flush());
        if let Some(source) = restored {
            if state.cancelled.contains(&(epoch, false)) {
                cleanup_transaction(state, epoch, false);
            } else {
                state.restored_sources.insert(epoch, source);
                cleanup_transaction(state, epoch, true);
            }
        } else {
            cleanup_transaction(state, epoch, false);
        }
        state.armed = None;
        Ok(())
    })();
    if result.is_err() {
        let keep_original = state.restored_sources.contains_key(&epoch);
        cleanup_transaction(state, epoch, keep_original);
    }
    result
}

fn cache_offer_eligible(cache_ready: bool, same_offer: bool) -> bool {
    cache_ready && same_offer
}

fn reject_unpublished_transaction(state: &mut State, epoch: u64) -> Result<()> {
    cleanup_transaction(state, epoch, false);
    bail!("clipboard changed before paste")
}

fn cleanup_transaction(state: &mut State, epoch: u64, keep_original: bool) {
    state.payloads.remove(&(epoch, true));
    if !keep_original {
        state.payloads.remove(&(epoch, false));
    }
    state.active.remove(&epoch);
    state.read_finished.remove(&epoch);
    state.active_transactions.remove(&epoch);
    state.cancelled.remove(&(epoch, true));
    state.cancelled.remove(&(epoch, false));
    if state.armed == Some(epoch) {
        state.armed = None;
    }
}

fn apply_transfer_result(state: &mut State, expected_epoch: u64, completed_epoch: u64, ok: bool) {
    if expected_epoch != completed_epoch {
        return;
    }
    let active = state.active.entry(expected_epoch).or_default();
    *active = active.saturating_sub(1);
    if state.armed == Some(expected_epoch) && ok && *active == 0 {
        state.read_finished.insert(expected_epoch, Instant::now());
    }
}

#[derive(Default)]
struct State {
    offers: HashMap<wayland_client::backend::ObjectId, Vec<String>>,
    clipboard: Option<offer::ExtDataControlOfferV1>,
    primary: Option<offer::ExtDataControlOfferV1>,
    cancelled: HashSet<(u64, bool)>,
    payloads: Payloads,
    restored_sources: HashMap<u64, source::ExtDataControlSourceV1>,
    armed: Option<u64>,
    active: HashMap<u64, usize>,
    read_finished: HashMap<u64, Instant>,
    next_epoch: u64,
    active_transactions: HashSet<u64>,
    transfers: Option<mpsc::Sender<(u64, bool)>>,
    errors: bool,
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<manager::ExtDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &manager::ExtDataControlManagerV1,
        _: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<offer::ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        object: &offer::ExtDataControlOfferV1,
        event: offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let offer::Event::Offer { mime_type } = event {
            state.offers.entry(object.id()).or_default().push(mime_type);
        }
    }
}
impl Dispatch<device::ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &device::ExtDataControlDeviceV1,
        event: device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            device::Event::Selection { id } => {
                if let Some(old) = state.clipboard.take() {
                    state.offers.remove(&old.id());
                    old.destroy();
                }
                state.clipboard = id;
            }
            device::Event::PrimarySelection { id } => {
                if let Some(old) = state.primary.take() {
                    state.offers.remove(&old.id());
                    old.destroy();
                }
                state.primary = id;
            }
            device::Event::Finished => state.errors = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, device::ExtDataControlDeviceV1, [
        0 => (offer::ExtDataControlOfferV1, ())
    ]);
}
impl Dispatch<source::ExtDataControlSourceV1, (u64, bool)> for State {
    fn event(
        state: &mut Self,
        source: &source::ExtDataControlSourceV1,
        event: source::Event,
        generation: &(u64, bool),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            source::Event::Cancelled => {
                // This source is no longer the active offer. The protocol
                // requires the client to release the server-side object too;
                // dropping the Rust proxy alone only forgets its object ID.
                source.destroy();
                state.payloads.remove(generation);
                if state.active_transactions.contains(&generation.0) {
                    state.cancelled.insert(*generation);
                }
                if !generation.1 {
                    state.restored_sources.remove(&generation.0);
                }
            }
            source::Event::Send { mime_type, fd } => {
                let payload = state
                    .payloads
                    .get(generation)
                    .and_then(|payloads| payloads.get(&mime_type))
                    .cloned();
                let content = state.armed == Some(generation.0)
                    && generation.1
                    && mime_type.starts_with("text/plain");
                if content {
                    *state.active.entry(generation.0).or_default() += 1;
                    state.read_finished.remove(&generation.0);
                }
                let sender = state.transfers.as_ref().unwrap().clone();
                let generation = generation.0;
                std::thread::spawn(move || {
                    let mut file = File::from(fd);
                    // Bound a stalled reader. SIGPIPE is ignored by Rust.
                    unsafe {
                        libc::fcntl(file.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
                    }
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut ok = true;
                    if let Some(payload) = payload {
                        let mut offset = 0;
                        while offset < payload.len() {
                            match file.write(&payload[offset..]) {
                                Ok(0) => {
                                    ok = false;
                                    break;
                                }
                                Ok(n) => offset += n,
                                Err(e)
                                    if e.kind() == std::io::ErrorKind::WouldBlock
                                        && Instant::now() < deadline =>
                                {
                                    std::thread::sleep(Duration::from_millis(5));
                                }
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                    }
                    drop(file); // EOF is part of completion, not just write success.
                    if content {
                        let _ = sender.send((generation, ok));
                    }
                });
            }
            _ => {}
        }
    }
}

fn pump(queue: &mut EventQueue<State>, state: &mut State, connection: &Connection) -> Result<()> {
    queue.dispatch_pending(state)?;
    connection.flush()?;
    if let Some(guard) = queue.prepare_read() {
        let mut poll = libc::pollfd {
            fd: connection.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 10) };
        if ready > 0 {
            guard.read()?;
        } else {
            drop(guard);
        }
    }
    queue.dispatch_pending(state)?;
    if state.errors {
        bail!("Wayland clipboard device disconnected");
    }
    Ok(())
}

fn backup(
    queue: &mut EventQueue<State>,
    state: &mut State,
    connection: &Connection,
    selected: Option<&offer::ExtDataControlOfferV1>,
) -> Result<HashMap<String, Arc<Vec<u8>>>> {
    let Some(selected) = selected else {
        return Ok(HashMap::new());
    };
    let formats = state
        .offers
        .get(&selected.id())
        .cloned()
        .unwrap_or_default();
    if formats.len() > 128 {
        bail!("clipboard has too many formats");
    }
    let mut transfers = Vec::new();
    for mime in formats {
        let mut fds = [0; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let input = unsafe { File::from_raw_fd(fds[0]) };
        let output = unsafe { File::from_raw_fd(fds[1]) };
        if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        selected.receive(mime.clone(), output.as_fd());
        transfers.push(BackupTransfer {
            mime,
            input,
            bytes: Vec::new(),
        });
    }
    connection.flush()?;
    read_backups(transfers, BACKUP_STALL_LIMIT, BACKUP_LIMIT, || {
        pump(queue, state, connection)?;
        if state.clipboard.as_ref() != Some(selected) && state.primary.as_ref() != Some(selected) {
            bail!("clipboard changed during backup; paste not started");
        }
        Ok(())
    })
}

struct BackupTransfer {
    mime: String,
    input: File,
    bytes: Vec<u8>,
}

fn read_backups(
    mut transfers: Vec<BackupTransfer>,
    stall_limit: Duration,
    total_limit: Duration,
    mut pump_events: impl FnMut() -> Result<()>,
) -> Result<HashMap<String, Arc<Vec<u8>>>> {
    let mut result = HashMap::new();
    let mut total = 0;
    let started = Instant::now();
    let mut progressed_at = started;
    let mut pumped_at = started;
    while !transfers.is_empty() {
        let mut progressed = false;
        let mut index = 0;
        while index < transfers.len() {
            let transfer = &mut transfers[index];
            let mut chunk = [0; 16384];
            match transfer.input.read(&mut chunk) {
                Ok(0) => {
                    let transfer = transfers.swap_remove(index);
                    result.insert(transfer.mime, Arc::new(transfer.bytes));
                    progressed = true;
                    continue;
                }
                Ok(count) => {
                    if transfer.bytes.len() + count > MAX_FORMAT || total + count > MAX_TOTAL {
                        bail!(
                            "clipboard backup exceeds safe memory limit at format {}",
                            transfer.mime
                        );
                    }
                    transfer.bytes.extend_from_slice(&chunk[..count]);
                    total += count;
                    progressed = true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("reading clipboard format {}", transfer.mime))
                }
            }
            index += 1;
        }
        if transfers.is_empty() {
            break;
        }
        let now = Instant::now();
        if progressed {
            progressed_at = now;
        }
        let stalled = now.duration_since(progressed_at) >= stall_limit;
        if stalled || now.duration_since(started) >= total_limit {
            let pending = transfers
                .iter()
                .take(4)
                .map(|t| t.mime.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "clipboard backup {} after {:.1}s; pending formats: {}; original clipboard kept",
                if stalled { "stalled" } else { "timed out" },
                now.duration_since(started).as_secs_f64(),
                pending
            );
        }
        if !progressed || now.duration_since(pumped_at) >= Duration::from_millis(10) {
            pump_events()?;
            pumped_at = Instant::now();
        }
    }
    Ok(result)
}

/// Child protocol: JSON text line, `ready`, then `arm`, `sent`/`abort`; reports `restored`
/// only after a complete transfer. Remains the original selection owner until a
/// later copy replaces it. No clipboard payload is placed in argv or on disk.
pub fn serve(primary: bool) -> Result<()> {
    let mut input = std::io::stdin().lock();
    let line = read_bounded_line(&mut input, MAX_FORMAT + 16)?;
    let text: String = serde_json::from_slice(&line)?;
    if text.len() > MAX_FORMAT || text.is_empty() {
        bail!("invalid clipboard transaction text");
    }
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let handle = queue.handle();
    let manager: manager::ExtDataControlManagerV1 = globals.bind(&handle, 1..=1, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=9, ())?;
    let device = manager.get_data_device(&seat, &handle, ());
    let (sender, receiver) = mpsc::channel();
    let mut state = State {
        transfers: Some(sender),
        ..State::default()
    };
    queue.roundtrip(&mut state)?;
    let original = if primary {
        state.primary.clone()
    } else {
        state.clipboard.clone()
    };
    let epoch = state.next_epoch;
    state.next_epoch = state.next_epoch.wrapping_add(1);
    state.cancelled.remove(&(epoch, true));
    state.cancelled.remove(&(epoch, false));
    state.active_transactions.insert(epoch);
    let original_payloads = backup(&mut queue, &mut state, &connection, original.as_ref())?;
    state.payloads.insert((epoch, false), original_payloads);
    queue.roundtrip(&mut state)?;
    let current = if primary {
        &state.primary
    } else {
        &state.clipboard
    };
    if current != &original {
        bail!("clipboard changed during backup; paste not started");
    }
    let temporary = manager.create_data_source(&handle, (epoch, true));
    let mut transcript_payloads = HashMap::new();
    for mime in [
        "text/plain;charset=utf-8",
        "text/plain",
        "x-kde-passwordManagerHint",
    ] {
        temporary.offer(mime.to_owned());
        transcript_payloads.insert(
            mime.to_owned(),
            Arc::new(if mime.starts_with("text/") {
                text.as_bytes().to_vec()
            } else {
                b"secret".to_vec()
            }),
        );
    }
    state.payloads.insert((epoch, true), transcript_payloads);
    let set_selection = |value: Option<&source::ExtDataControlSourceV1>| {
        if primary {
            device.set_primary_selection(value);
        } else {
            device.set_selection(value);
        }
    };
    set_selection(Some(&temporary));
    queue.roundtrip(&mut state)?;
    if state.cancelled.contains(&(epoch, true)) {
        bail!("clipboard changed before paste");
    }
    let mut output = std::io::stdout();
    let parent_ready = writeln!(output, "ready")
        .and_then(|()| output.flush())
        .is_ok();
    // Parent commands must not stop dispatching Wayland transfers.
    drop(input);
    let (commands, command_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if commands.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = commands.send("abort".into());
    });
    let mut failed = !parent_ready;
    let preparation_deadline = Instant::now() + Duration::from_secs(60);
    let mut sent_at = None;
    loop {
        while let Ok(command) = command_rx.try_recv() {
            if command == "arm" {
                state.armed = Some(epoch);
                state.read_finished.remove(&epoch);
                if writeln!(output, "armed")
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    failed = true;
                }
            } else if command == "sent" {
                sent_at = Some(Instant::now());
            } else {
                failed = true;
            }
        }
        pump(&mut queue, &mut state, &connection)?;
        if state.cancelled.contains(&(epoch, true)) {
            println!("replaced");
            cleanup_transaction(&mut state, epoch, false);
            return Ok(());
        }
        while let Ok((transfer_epoch, ok)) = receiver.try_recv() {
            apply_transfer_result(&mut state, epoch, transfer_epoch, ok);
            if transfer_epoch == epoch && !ok {
                failed = true;
            }
        }
        let complete = sent_at.is_some_and(|at: Instant| at.elapsed() >= SETTLE)
            && state.active.get(&epoch).copied().unwrap_or(0) == 0
            && state
                .read_finished
                .get(&epoch)
                .is_some_and(|at| at.elapsed() >= SETTLE);
        let timed_out = sent_at.map_or_else(
            || Instant::now() >= preparation_deadline,
            |at| at.elapsed() >= READ_LIMIT,
        );
        if failed || timed_out || complete {
            failed |= !complete;
            // Dispatch pending selection cancellation before touching ownership.
            queue.roundtrip(&mut state)?;
            if state.cancelled.contains(&(epoch, true)) {
                println!("replaced");
                cleanup_transaction(&mut state, epoch, false);
                return Ok(());
            }
            break;
        }
    }
    let restored_payloads = state.payloads.get(&(epoch, false));
    let restored = if restored_payloads.is_none_or(HashMap::is_empty) {
        None
    } else {
        let source = manager.create_data_source(&handle, (epoch, false));
        for mime in restored_payloads.unwrap().keys() {
            source.offer(mime.clone());
        }
        Some(source)
    };
    set_selection(restored.as_ref());
    queue.roundtrip(&mut state)?;
    let _ = writeln!(output, "{}", if failed { "unread" } else { "restored" });
    let _ = output.flush();
    if restored.is_none() {
        cleanup_transaction(&mut state, epoch, false);
        return Ok(());
    }
    while !state.cancelled.contains(&(epoch, false)) {
        pump(&mut queue, &mut state, &connection)?;
    }
    state.payloads.remove(&(epoch, true));
    state.payloads.remove(&(epoch, false));
    state.active.remove(&epoch);
    state.read_finished.remove(&epoch);
    state.active_transactions.remove(&epoch);
    state.armed = None;
    Ok(())
}

#[cfg(test)]
mod backup_tests {
    use super::*;

    fn pipe(mime: &str) -> (BackupTransfer, File) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let input = unsafe { File::from_raw_fd(fds[0]) };
        let output = unsafe { File::from_raw_fd(fds[1]) };
        assert_ne!(
            unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
            -1
        );
        (
            BackupTransfer {
                mime: mime.into(),
                input,
                bytes: Vec::new(),
            },
            output,
        )
    }

    #[test]
    fn cache_status_is_metadata_only_and_uses_expected_field_names() {
        let status = CacheStatus {
            generation: 3,
            ready: true,
            warming: false,
            formats: 2,
            cached_bytes: 17,
            ..CacheStatus::default()
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["generation"], 3);
        assert_eq!(value["ready"], true);
        assert_eq!(value["warming"], false);
        assert_eq!(value["formats"], 2);
        assert_eq!(value["bytes"], 17);
        assert!(value.get("cached_bytes").is_none());
    }

    #[test]
    fn phase_events_are_bounded_and_survive_status_updates() {
        let status = std::sync::Mutex::new(CacheStatus {
            instance: 123,
            ..CacheStatus::default()
        });
        for generation in 1..=20 {
            publish_event(&status, generation, CachePhase::Buffering);
            publish_event(&status, generation, CachePhase::Ready);
        }
        publish_status(
            &status,
            CacheStatus {
                generation: 20,
                ready: true,
                ..CacheStatus::default()
            },
        );
        let status = status.lock().unwrap();
        assert_eq!(status.instance, 123);
        assert_eq!(status.events.len(), 32);
        assert_eq!(status.events.first().unwrap().sequence, 9);
        assert_eq!(status.events.last().unwrap().sequence, 40);
        assert_eq!(status.events.last().unwrap().phase, CachePhase::Ready);
        let json = serde_json::to_string(&*status).unwrap();
        assert!(json.len() < 16384);
        assert!(!json.contains("text"));
    }

    #[test]
    fn late_transfer_completion_cannot_change_a_new_transaction() {
        let mut state = State {
            armed: Some(8),
            ..State::default()
        };
        state.active.insert(8, 2);
        apply_transfer_result(&mut state, 8, 7, false);
        assert_eq!(state.active.get(&8), Some(&2));
        assert!(!state.read_finished.contains_key(&8));
        apply_transfer_result(&mut state, 8, 8, true);
        assert_eq!(state.active.get(&8), Some(&1));
        assert!(!state.read_finished.contains_key(&8));
    }

    #[test]
    fn failed_or_partial_warm_cache_is_never_eligible_for_restore() {
        assert!(!cache_offer_eligible(false, true));
        assert!(!cache_offer_eligible(false, false));
        assert!(cache_offer_eligible(true, true));
        assert!(!cache_offer_eligible(true, false));
    }

    #[test]
    fn superseded_offer_drops_all_pending_pipe_read_ends() {
        let (first, mut first_writer) = pipe("image/one");
        let (second, mut second_writer) = pipe("image/two");
        let error = read_backups(
            vec![first, second],
            Duration::from_secs(1),
            Duration::from_secs(2),
            || bail!("clipboard changed during backup"),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("clipboard changed"), "{error}");
        for writer in [&mut first_writer, &mut second_writer] {
            let error = writer.write_all(b"late data").unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
    }

    #[test]
    fn transaction_cleanup_retains_only_the_live_original_until_next_cleanup() {
        let mut state = State {
            armed: Some(12),
            ..State::default()
        };
        state.active.insert(12, 1);
        state.read_finished.insert(12, Instant::now());
        state.active_transactions.insert(12);
        state.cancelled.insert((12, true));
        state.payloads.insert(
            (12, false),
            HashMap::from([("image/png".to_owned(), Arc::new(vec![1, 2, 3]))]),
        );
        state.payloads.insert(
            (12, true),
            HashMap::from([("text/plain".to_owned(), Arc::new(b"transcript".to_vec()))]),
        );

        cleanup_transaction(&mut state, 12, true);

        assert_eq!(state.payloads.len(), 1);
        assert!(state.payloads.contains_key(&(12, false)));
        assert!(!state.payloads.contains_key(&(12, true)));
        assert!(state.active.is_empty());
        assert!(state.read_finished.is_empty());
        assert!(state.active_transactions.is_empty());
        assert!(state.cancelled.is_empty());
        assert_eq!(state.armed, None);

        state.payloads.insert(
            (13, false),
            HashMap::from([("text/plain".to_owned(), Arc::new(b"latest".to_vec()))]),
        );
        cleanup_transaction(&mut state, 12, false);
        cleanup_transaction(&mut state, 13, false);
        assert!(state.payloads.is_empty());
    }

    #[test]
    fn replaced_transaction_before_paste_cleans_its_epoch() {
        let mut state = State::default();
        state.active_transactions.insert(21);
        state.active.insert(21, 1);
        state.read_finished.insert(21, Instant::now());
        state.armed = Some(21);
        state.cancelled.insert((21, true));
        state.payloads.insert(
            (21, false),
            HashMap::from([("image/png".to_owned(), Arc::new(vec![0; 8]))]),
        );
        state.payloads.insert(
            (21, true),
            HashMap::from([("text/plain".to_owned(), Arc::new(b"temporary".to_vec()))]),
        );

        let error = reject_unpublished_transaction(&mut state, 21).unwrap_err();

        assert!(error.to_string().contains("clipboard changed before paste"));
        assert!(state.payloads.is_empty());
        assert!(state.active_transactions.is_empty());
        assert!(state.active.is_empty());
        assert!(state.read_finished.is_empty());
        assert!(state.cancelled.is_empty());
        assert_eq!(state.armed, None);
    }

    #[test]
    fn queued_slow_formats_preserve_all_bytes_without_a_shared_short_deadline() {
        let mut transfers = Vec::new();
        let mut writers = Vec::new();
        for mime in [
            "image/eps",
            "image/epsf",
            "image/epsi",
            "application/x-custom",
        ] {
            let (transfer, writer) = pipe(mime);
            transfers.push(transfer);
            writers.push(writer);
        }
        let writer = std::thread::spawn(move || {
            for (index, mut output) in writers.into_iter().enumerate() {
                std::thread::sleep(Duration::from_millis(80));
                output.write_all(&[index as u8, 0, 255]).unwrap();
            }
        });
        let result = read_backups(
            transfers,
            Duration::from_millis(250),
            Duration::from_secs(2),
            || {
                std::thread::sleep(Duration::from_millis(2));
                Ok(())
            },
        )
        .unwrap();
        writer.join().unwrap();
        for (index, mime) in [
            "image/eps",
            "image/epsf",
            "image/epsi",
            "application/x-custom",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(result[*mime].as_slice(), &[index as u8, 0, 255]);
        }
    }

    #[test]
    fn unfinished_transfer_names_the_stalled_format_instead_of_reporting_eagain() {
        let (transfer, _held_open) = pipe("image/eps");
        let error = read_backups(
            vec![transfer],
            Duration::from_millis(30),
            Duration::from_secs(1),
            || {
                std::thread::sleep(Duration::from_millis(2));
                Ok(())
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("stalled"), "{error}");
        assert!(error.contains("image/eps"), "{error}");
        assert!(error.contains("original clipboard kept"), "{error}");
    }

    #[test]
    fn other_completed_formats_do_not_hide_an_unfinished_transfer() {
        let (stalled, _held_open) = pipe("application/x-stalled");
        let (done, mut writer) = pipe("text/plain");
        writer.write_all(b"complete").unwrap();
        drop(writer);
        assert!(read_backups(
            vec![stalled, done],
            Duration::from_millis(30),
            Duration::from_secs(1),
            || {
                std::thread::sleep(Duration::from_millis(2));
                Ok(())
            }
        )
        .unwrap_err()
        .to_string()
        .contains("application/x-stalled"));
    }
    #[test]
    fn ongoing_data_does_not_remove_the_total_time_limit() {
        let (transfer, mut output) = pipe("application/x-stream");
        let writer = std::thread::spawn(move || {
            for _ in 0..100 {
                if output.write_all(b"more").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let error = read_backups(
            vec![transfer],
            Duration::from_secs(1),
            Duration::from_millis(60),
            || {
                std::thread::sleep(Duration::from_millis(2));
                Ok(())
            },
        )
        .unwrap_err()
        .to_string();
        writer.join().unwrap();
        assert!(error.contains("timed out"), "{error}");
        assert!(error.contains("application/x-stream"), "{error}");
    }
}
