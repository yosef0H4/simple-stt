//! A temporary Wayland selection with a complete, bounded backup. Restoration
//! follows completed payload transfers, never an unconditional post-key timer.
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
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
const SETTLE: Duration = Duration::from_millis(250);
const READ_LIMIT: Duration = Duration::from_secs(8);

#[derive(Default)]
struct State {
    offers: HashMap<wayland_client::backend::ObjectId, Vec<String>>,
    clipboard: Option<offer::ExtDataControlOfferV1>,
    primary: Option<offer::ExtDataControlOfferV1>,
    cancelled: [bool; 2],
    payloads: [HashMap<String, Arc<Vec<u8>>>; 2],
    armed: bool,
    active: usize,
    read_finished: Option<Instant>,
    transfers: Option<mpsc::Sender<(usize, bool)>>,
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
impl Dispatch<source::ExtDataControlSourceV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &source::ExtDataControlSourceV1,
        event: source::Event,
        generation: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            source::Event::Cancelled => state.cancelled[*generation] = true,
            source::Event::Send { mime_type, fd } => {
                let payload = state.payloads[*generation].get(&mime_type).cloned();
                let content =
                    state.armed && *generation == 0 && mime_type.starts_with("text/plain");
                if content {
                    state.active += 1;
                    state.read_finished = None;
                }
                let sender = state.transfers.as_ref().unwrap().clone();
                let generation = *generation;
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
    let mut result = HashMap::new();
    let mut total = 0;
    let backup_deadline = Instant::now() + Duration::from_secs(5);
    for mime in formats {
        if Instant::now() >= backup_deadline {
            bail!("clipboard backup timed out");
        }
        let mut fds = [0; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut input = unsafe { File::from_raw_fd(fds[0]) };
        let output = unsafe { File::from_raw_fd(fds[1]) };
        unsafe {
            libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }
        selected.receive(mime.clone(), output.as_fd());
        connection.flush()?;
        drop(output);
        let deadline = (Instant::now() + Duration::from_secs(3)).min(backup_deadline);
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 16384];
            match input.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > MAX_FORMAT || total + bytes.len() > MAX_TOTAL {
                        bail!("clipboard backup exceeds safe memory limit");
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    pump(queue, state, connection)?;
                }
                Err(e) => return Err(e).context("reading complete clipboard backup"),
            }
        }
        total += bytes.len();
        result.insert(mime, Arc::new(bytes));
    }
    Ok(result)
}

/// Child protocol: JSON text line, `ready`, then `arm`, `sent`/`abort`; reports `restored`
/// only after a complete transfer. Remains the original selection owner until a
/// later copy replaces it. No clipboard payload is placed in argv or on disk.
pub fn serve(primary: bool) -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut line = String::new();
    input.read_line(&mut line)?;
    let text: String = serde_json::from_str(&line)?;
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
    state.payloads[1] = backup(&mut queue, &mut state, &connection, original.as_ref())?;
    queue.roundtrip(&mut state)?;
    let current = if primary {
        &state.primary
    } else {
        &state.clipboard
    };
    if current != &original {
        bail!("clipboard changed during backup; paste not started");
    }
    let temporary = manager.create_data_source(&handle, 0);
    for mime in [
        "text/plain;charset=utf-8",
        "text/plain",
        "x-kde-passwordManagerHint",
    ] {
        temporary.offer(mime.to_owned());
        state.payloads[0].insert(
            mime.to_owned(),
            Arc::new(if mime.starts_with("text/") {
                text.as_bytes().to_vec()
            } else {
                b"secret".to_vec()
            }),
        );
    }
    let set_selection = |value: Option<&source::ExtDataControlSourceV1>| {
        if primary {
            device.set_primary_selection(value);
        } else {
            device.set_selection(value);
        }
    };
    set_selection(Some(&temporary));
    queue.roundtrip(&mut state)?;
    if state.cancelled[0] {
        bail!("clipboard changed before paste");
    }
    let parent_ready = writeln!(std::io::stdout(), "ready")
        .and_then(|()| std::io::stdout().flush())
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
                state.armed = true;
                state.read_finished = None;
                if writeln!(std::io::stdout(), "armed")
                    .and_then(|()| std::io::stdout().flush())
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
        if state.cancelled[0] {
            println!("replaced");
            return Ok(());
        }
        while let Ok((_, ok)) = receiver.try_recv() {
            state.active = state.active.saturating_sub(1);
            if state.armed && ok && state.active == 0 {
                state.read_finished = Some(Instant::now());
            }
            if !ok {
                failed = true;
            }
        }
        let complete = sent_at.is_some_and(|at: Instant| at.elapsed() >= SETTLE)
            && state.active == 0
            && state.read_finished.is_some_and(|at| at.elapsed() >= SETTLE);
        let timed_out = sent_at.map_or_else(
            || Instant::now() >= preparation_deadline,
            |at| at.elapsed() >= READ_LIMIT,
        );
        if failed || timed_out || complete {
            failed |= !complete;
            // Dispatch pending selection cancellation before touching ownership.
            queue.roundtrip(&mut state)?;
            if state.cancelled[0] {
                println!("replaced");
                return Ok(());
            }
            break;
        }
    }
    let restored = if state.payloads[1].is_empty() {
        None
    } else {
        let source = manager.create_data_source(&handle, 1);
        for mime in state.payloads[1].keys() {
            source.offer(mime.clone());
        }
        Some(source)
    };
    set_selection(restored.as_ref());
    queue.roundtrip(&mut state)?;
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        if failed { "unread" } else { "restored" }
    );
    let _ = std::io::stdout().flush();
    if restored.is_none() {
        return Ok(());
    }
    while !state.cancelled[1] {
        pump(&mut queue, &mut state, &connection)?;
    }
    Ok(())
}
