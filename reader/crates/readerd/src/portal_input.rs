//! Local desktop input broker. Never exposed through the HTTP inference API.
//! All Linux injection uses a compositor-authorized RemoteDesktop EIS session.
use anyhow::{bail, ensure, Context, Result};
use ashpd::desktop::{
    remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions},
    PersistMode, Session,
};
use futures_lite::future::{block_on, poll_once};
use reis::{
    ei,
    event::{Device, DeviceCapability as Cap, EiEvent, EiEventConverter},
    PendingRequestResult,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    io::Write,
    os::unix::{fs::OpenOptionsExt, net::UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Prepare,
    PreparePointer,
    Key {
        key: String,
    },
    Sequence {
        sequence: reader_core::recorded_action::RecordedAction,
    },
    Text {
        text: String,
        delay_ms: u64,
    },
    Click {
        x: i32,
        y: i32,
    },
}
struct Task {
    action: Action,
    canceled: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Result<()>>,
}
static BROKER: OnceLock<mpsc::SyncSender<Task>> = OnceLock::new();
type Requests = HashMap<String, (Instant, Arc<AtomicBool>)>;
static REQUESTS: OnceLock<Mutex<Requests>> = OnceLock::new();

pub fn cancel(id: &str) {
    let mut requests = REQUESTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    requests.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(660));
    requests
        .entry(id.to_owned())
        .or_insert_with(|| (Instant::now(), Arc::new(AtomicBool::new(false))))
        .1
        .store(true, Ordering::Release);
}
pub fn local(id: String, action: Action) -> Result<()> {
    ensure!(id.len() <= 64, "invalid input request id");
    let canceled = {
        let mut requests = REQUESTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        requests.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(660));
        ensure!(requests.len() < 32, "too many input requests");
        requests
            .entry(id.clone())
            .or_insert_with(|| (Instant::now(), Arc::new(AtomicBool::new(false))))
            .1
            .clone()
    };
    let result = run(action, &|| !canceled.load(Ordering::Acquire));
    REQUESTS
        .get()
        .unwrap()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
    result
}
pub fn run(action: Action, current: &dyn Fn() -> bool) -> Result<()> {
    // reis's explicit protocol dump includes text. Do not allow an inherited
    // debug variable to bypass the application's content-log privacy setting.
    ensure!(
        std::env::var_os("REIS_DEBUG").is_none_or(|v| v.is_empty())
            || crate::privacy::content_logging(),
        "REIS_DEBUG requires privacy.log_content; unset it for private input"
    );
    if let Action::Key { key } = &action {
        chord(key)?;
    }
    if let Action::Sequence { sequence } = &action {
        sequence.validate()?;
    }
    if let Action::Text { text, delay_ms } = &action {
        ensure!(
            text.len() <= 48 * 1024 && *delay_ms <= 1000,
            "input text or pacing exceeds limit"
        );
        ensure!(!text.contains('\0'), "input contains a null character");
    }
    let broker = BROKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Task>(1);
        std::thread::Builder::new()
            .name("portal-input".into())
            .spawn(move || {
                let mut keyboard: Option<Connection> = None;
                let mut pointer: Option<Connection> = None;
                loop {
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(task) => {
                            let started = Instant::now();
                            let current = || {
                                !task.canceled.load(Ordering::Acquire)
                                    && started.elapsed() < Duration::from_secs(600)
                            };
                            let is_pointer = matches!(
                                task.action,
                                Action::Click { .. } | Action::PreparePointer
                            );
                            let slot = if is_pointer {
                                &mut pointer
                            } else {
                                &mut keyboard
                            };
                            let result = (|| {
                                let mut focus = if matches!(
                                    task.action,
                                    Action::Prepare | Action::PreparePointer
                                ) {
                                    None
                                } else {
                                    Some(crate::read_along_capture::InputFocus::capture()?)
                                };
                                ensure!(current(), "input canceled before delivery");
                                if slot.is_none() {
                                    *slot = Some(Connection::connect(is_pointer, &current)?);
                                }
                                slot.as_mut()
                                    .unwrap()
                                    .execute(&task.action, &current, &mut focus)
                            })();
                            // Never replay after an uncertain partial delivery.
                            if result.is_err() {
                                *slot = None;
                            }
                            let _ = task.reply.send(result);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            for slot in [&mut keyboard, &mut pointer] {
                                if slot
                                    .as_mut()
                                    .is_some_and(|connection| connection.pump().is_err())
                                {
                                    *slot = None;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .expect("starting portal input broker");
        tx
    });
    let canceled = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(1);
    let mut task = Task {
        action,
        canceled: canceled.clone(),
        reply: tx,
    };
    let deadline = Instant::now() + Duration::from_secs(610);
    loop {
        ensure!(current(), "input canceled before queueing");
        match broker.try_send(task) {
            Ok(()) => break,
            Err(mpsc::TrySendError::Full(pending)) => task = pending,
            Err(_) => bail!("portal input broker stopped"),
        }
        ensure!(Instant::now() < deadline, "input queue timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        if !current() || Instant::now() >= deadline {
            canceled.store(true, Ordering::Release);
            bail!("input canceled; text may be partially delivered");
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => bail!("portal input broker stopped"),
        }
    }
}
fn request<T>(
    future: impl Future<Output = ashpd::Result<T>>,
    current: &dyn Fn() -> bool,
) -> Result<T> {
    let mut future = std::pin::pin!(future);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        ensure!(current(), "portal input canceled");
        ensure!(
            Instant::now() < deadline,
            "desktop-control permission timed out"
        );
        if let Some(result) = block_on(poll_once(future.as_mut())) {
            return result.context("RemoteDesktop input");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
// ashpd 0.13 Session has no Drop cleanup. Own the remote resource from the
// instant CreateSession succeeds, including every subsequent setup error.
struct SessionLease(Session<RemoteDesktop>);
impl Drop for SessionLease {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        if let Err(error) = request(self.0.close(), &|| Instant::now() < deadline) {
            tracing::debug!(%error, "closing desktop-control session");
        }
    }
}
struct Connection {
    _portal: RemoteDesktop,
    _session: SessionLease,
    context: ei::Context,
    events: EiEventConverter,
    active: Vec<Device>,
    sequence: u32,
    clock: Instant,
}
impl Connection {
    fn connect(pointer: bool, current: &dyn Fn() -> bool) -> Result<Self> {
        let portal = request(RemoteDesktop::new(), current)?;
        // CreateSession has no consent dialog. Obtain its handle even if this
        // task is canceled meanwhile, so the lease can close it deterministically.
        // Later consent/handshake waits remain immediately cancellable.
        let session = SessionLease(request(portal.create_session(Default::default()), &|| {
            true
        })?);
        let path = reader_core::config::data_dir().join(if pointer {
            "remote-desktop-pointer-token"
        } else {
            "remote-desktop-keyboard-token"
        });
        std::fs::create_dir_all(reader_core::config::data_dir())?;
        let token = std::fs::read_to_string(&path).ok();
        let device = if pointer {
            DeviceType::Pointer
        } else {
            DeviceType::Keyboard
        };
        request(
            portal.select_devices(
                &session.0,
                SelectDevicesOptions::default()
                    .set_devices(Some(device.into()))
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref().map(str::trim)),
            ),
            current,
        )?
        .response()?;
        let granted =
            request(portal.start(&session.0, None, Default::default()), current)?.response()?;
        ensure!(
            granted.devices().contains(device),
            "desktop-control permission was not granted"
        );
        if let Some(token) = granted.restore_token() {
            let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            let result = (|| -> std::io::Result<()> {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?;
                file.write_all(token.as_bytes())?;
                file.sync_all()?;
                std::fs::rename(&temporary, &path)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(temporary);
            }
            result.context("remembering desktop-control permission")?;
        }
        let fd = request(
            portal.connect_to_eis(&session.0, Default::default()),
            current,
        )
        .context("This desktop must support RemoteDesktop ConnectToEIS")?;
        let context = ei::Context::new(UnixStream::from(fd))?;
        let mut handshake =
            reis::handshake::EiHandshaker::new("Simple STT", ei::handshake::ContextType::Sender);
        let deadline = Instant::now() + Duration::from_secs(5);
        let response = 'handshake: loop {
            ensure!(
                current() && Instant::now() < deadline,
                "portal input handshake timed out or canceled"
            );
            read(&context)?;
            while let Some(event) = context.pending_event() {
                if let Some(response) = handshake.handle_event(unpack(event)?)? {
                    break 'handshake response;
                }
            }
            context.flush()?;
            std::thread::sleep(Duration::from_millis(5));
        };
        let events = EiEventConverter::new(&context, response);
        Ok(Self {
            _portal: portal,
            _session: session,
            context,
            events,
            active: Vec::new(),
            sequence: 0,
            clock: Instant::now(),
        })
    }
    fn pump(&mut self) -> Result<()> {
        read(&self.context)?;
        while let Some(event) = self.context.pending_event() {
            self.events.handle_event(unpack(event)?)?;
        }
        while let Some(event) = self.events.next_event() {
            match event {
                EiEvent::SeatAdded(event) => event.seat.bind_capabilities(
                    Cap::Keyboard | Cap::Text | Cap::PointerAbsolute | Cap::Button,
                ),
                EiEvent::DeviceResumed(event) => {
                    self.active.retain(|d| d != &event.device);
                    self.active.push(event.device);
                }
                EiEvent::DevicePaused(event) => self.active.retain(|d| d != &event.device),
                EiEvent::DeviceRemoved(event) => self.active.retain(|d| d != &event.device),
                EiEvent::Disconnected(_) => bail!("desktop-control session disconnected"),
                _ => {}
            }
        }
        self.context.flush()?;
        Ok(())
    }
    fn device(&mut self, capability: Cap, current: &dyn Fn() -> bool) -> Result<Device> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pump()?;
            if let Some(device) = self.active.iter().find(|d| d.has_capability(capability)) {
                return Ok(device.clone());
            }
            ensure!(
                current() && Instant::now() < deadline,
                "desktop does not offer the requested input capability, or input canceled"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn frame(&self, device: &Device) {
        device.device().frame(
            self.events.connection().serial(),
            self.clock.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        );
    }
    fn sync(&mut self, current: &dyn Fn() -> bool) -> Result<()> {
        let done = Arc::new(AtomicBool::new(false));
        let signal = done.clone();
        let callback = self.events.connection().connection().sync(1);
        self.events.add_callback_handler(callback, move |_| {
            signal.store(true, Ordering::Release);
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while !done.load(Ordering::Acquire) {
            self.pump()?;
            ensure!(
                current() && Instant::now() < deadline,
                "desktop input acknowledgment timed out or canceled"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    fn keys(&mut self, codes: &[u32], current: &dyn Fn() -> bool) -> Result<()> {
        let device = self.device(Cap::Keyboard, current)?;
        ensure!(current(), "input canceled before key press");
        self.sequence = self.sequence.wrapping_add(1);
        device
            .device()
            .start_emulating(self.events.connection().serial(), self.sequence);
        let keyboard = device
            .interface::<ei::Keyboard>()
            .context("keyboard interface unavailable")?;
        for code in codes {
            keyboard.key(*code, ei::keyboard::KeyState::Press);
        }
        self.frame(&device);
        let pressed = self.sync(current);
        for code in codes.iter().rev() {
            keyboard.key(*code, ei::keyboard::KeyState::Released);
        }
        self.frame(&device);
        device
            .device()
            .stop_emulating(self.events.connection().serial());
        // Even a canceled command must release every modifier/key.
        let released = self.sync(&|| true);
        pressed?;
        released
    }
    fn recorded(
        &mut self,
        sequence: &reader_core::recorded_action::RecordedAction,
        current: &dyn Fn() -> bool,
        focus: &mut Option<crate::read_along_capture::InputFocus>,
    ) -> Result<()> {
        use reader_core::recorded_action::{keycode, replay, Playback};
        ensure!(
            focus.as_ref().is_some_and(|focus| focus.has_observer()),
            "recorded actions require an observable focused window on this desktop"
        );
        let device = self.device(Cap::Keyboard, current)?;
        let keyboard = device
            .interface::<ei::Keyboard>()
            .context("keyboard interface unavailable")?;
        self.sequence = self.sequence.wrapping_add(1);
        device
            .device()
            .start_emulating(self.events.connection().serial(), self.sequence);
        struct Output<'a> {
            connection: &'a mut Connection,
            device: &'a Device,
            keyboard: &'a ei::Keyboard,
            current: &'a dyn Fn() -> bool,
            focus: &'a mut Option<crate::read_along_capture::InputFocus>,
            started: Instant,
        }
        impl Playback for Output<'_> {
            fn wait_until(&mut self, at_ms: u32) -> Result<()> {
                loop {
                    ensure!((self.current)(), "recorded action canceled");
                    if let Some(focus) = self.focus.as_mut() {
                        focus.check()?;
                    }
                    self.connection.pump()?;
                    ensure!(
                        self.connection.active.contains(self.device),
                        "keyboard device paused or removed"
                    );
                    let remaining = Duration::from_millis(u64::from(at_ms))
                        .saturating_sub(self.started.elapsed());
                    if remaining.is_zero() {
                        return Ok(());
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(5)));
                }
            }
            fn key(&mut self, key: &str, down: bool) -> Result<()> {
                self.keyboard.key(
                    keycode(key)?,
                    if down {
                        ei::keyboard::KeyState::Press
                    } else {
                        ei::keyboard::KeyState::Released
                    },
                );
                self.connection.frame(self.device);
                self.connection.context.flush()?;
                Ok(())
            }
        }
        let result = replay(
            sequence,
            &mut Output {
                connection: self,
                device: &device,
                keyboard: &keyboard,
                current,
                focus,
                started: Instant::now(),
            },
        );
        device
            .device()
            .stop_emulating(self.events.connection().serial());
        let released = self.sync(&|| true);
        result.and(released)
    }
    fn execute(
        &mut self,
        action: &Action,
        current: &dyn Fn() -> bool,
        focus: &mut Option<crate::read_along_capture::InputFocus>,
    ) -> Result<()> {
        if let Some(focus) = focus {
            focus.check()?;
        }
        match action {
            Action::PreparePointer => {
                self.device(Cap::PointerAbsolute, current)?;
                self.device(Cap::Button, current)?;
                Ok(())
            }
            Action::Prepare => {
                self.device(Cap::Keyboard, current)?;
                Ok(())
            }
            Action::Key { key } => self.keys(&chord(key)?, current),
            Action::Sequence { sequence } => self.recorded(sequence, current, focus),
            Action::Text { text, delay_ms } => {
                let device = self.device(Cap::Text, current).context("Unicode typing requires the compositor's EIS text support; choose a paste action on older desktops")?;
                let input = device
                    .interface::<ei::Text>()
                    .context("EIS text interface unavailable")?;
                for character in text.chars() {
                    if let Some(focus) = focus {
                        focus.check()?;
                    }
                    ensure!(
                        current(),
                        "typing canceled; text may be partially delivered"
                    );
                    if matches!(character, '\n' | '\t' | '\r') {
                        if character != '\r' {
                            self.keys(&[if character == '\n' { 28 } else { 15 }], current)?;
                        }
                    } else {
                        self.sequence = self.sequence.wrapping_add(1);
                        device
                            .device()
                            .start_emulating(self.events.connection().serial(), self.sequence);
                        input.utf8(character.encode_utf8(&mut [0; 4]));
                        self.frame(&device);
                        device
                            .device()
                            .stop_emulating(self.events.connection().serial());
                        self.sync(current)?;
                    }
                    let until = Instant::now() + Duration::from_millis(*delay_ms);
                    while Instant::now() < until {
                        ensure!(current(), "typing canceled");
                        self.pump()?;
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                Ok(())
            }
            Action::Click { x, y } => {
                let device = self.device(Cap::PointerAbsolute, current)?;
                // EIS regions use compositor logical coordinates, including scale.
                ensure!(
                    device
                        .regions()
                        .iter()
                        .any(|r| i64::from(*x) >= i64::from(r.x)
                            && i64::from(*y) >= i64::from(r.y)
                            && i64::from(*x) < i64::from(r.x) + i64::from(r.width)
                            && i64::from(*y) < i64::from(r.y) + i64::from(r.height)),
                    "click is outside compositor input regions"
                );
                let button_device = self.device(Cap::Button, current)?;
                let pointer = device
                    .interface::<ei::PointerAbsolute>()
                    .context("pointer interface unavailable")?;
                let button = button_device
                    .interface::<ei::Button>()
                    .context("button interface unavailable")?;
                self.sequence = self.sequence.wrapping_add(1);
                device
                    .device()
                    .start_emulating(self.events.connection().serial(), self.sequence);
                pointer.motion_absolute(*x as f32, *y as f32);
                self.frame(&device);
                self.sync(current)?;
                if button_device != device {
                    button_device
                        .device()
                        .start_emulating(self.events.connection().serial(), self.sequence);
                }
                button.button(0x110, ei::button::ButtonState::Press);
                self.frame(&button_device);
                let pressed = self.sync(current);
                button.button(0x110, ei::button::ButtonState::Released);
                self.frame(&button_device);
                if button_device != device {
                    button_device
                        .device()
                        .stop_emulating(self.events.connection().serial());
                }
                device
                    .device()
                    .stop_emulating(self.events.connection().serial());
                let released = self.sync(&|| true);
                pressed?;
                released
            }
        }
    }
}
fn read(context: &ei::Context) -> Result<()> {
    match context.read() {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn unpack(event: PendingRequestResult<ei::Event>) -> Result<ei::Event> {
    match event {
        PendingRequestResult::Request(event) => Ok(event),
        _ => bail!("invalid compositor input protocol event"),
    }
}
/// Physical shortcut names; transcript Unicode is delivered via ei_text instead.
fn chord(value: &str) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    let parts: Vec<_> = value
        .split('+')
        .map(|v| v.trim().to_ascii_lowercase())
        .collect();
    ensure!(!parts.is_empty() && parts.len() <= 5, "invalid key chord");
    for (index, part) in parts.iter().enumerate() {
        let code = match part.as_str() {
            "ctrl"|"control" => 29, "shift"=>42, "alt"=>56, "meta"|"super"|"win"=>125,
            "space"=>57, "enter"|"return"=>28, "tab"=>15, "escape"|"esc"=>1,
            "backspace"=>14, "insert"=>110, "delete"=>111, "home"=>102, "end"=>107,
            "pageup"|"pgup"=>104, "pagedown"|"pgdown"|"pgdn"=>109,
            "left"=>105,"right"=>106,"up"=>103,"down"=>108,
            "a"=>30,"b"=>48,"c"=>46,"d"=>32,"e"=>18,"f"=>33,"g"=>34,"h"=>35,"i"=>23,"j"=>36,"k"=>37,"l"=>38,"m"=>50,"n"=>49,"o"=>24,"p"=>25,"q"=>16,"r"=>19,"s"=>31,"t"=>20,"u"=>22,"v"=>47,"w"=>17,"x"=>45,"y"=>21,"z"=>44,
            "1"=>2,"2"=>3,"3"=>4,"4"=>5,"5"=>6,"6"=>7,"7"=>8,"8"=>9,"9"=>10,"0"=>11,
            "f1"=>59,"f2"=>60,"f3"=>61,"f4"=>62,"f5"=>63,"f6"=>64,"f7"=>65,"f8"=>66,"f9"=>67,"f10"=>68,"f11"=>87,"f12"=>88,
            _=>bail!("unsupported shortcut key; use a letter, digit, F1–F12, navigation key, or modifiers joined with +"),
        };
        ensure!(!result.contains(&code), "duplicate key in chord");
        ensure!(
            index + 1 == parts.len() || matches!(code, 29 | 42 | 56 | 125),
            "only modifiers may precede the final shortcut key"
        );
        result.push(code);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn idle_eis_socket_is_not_a_disconnect() {
        let (client, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let context = reis::ei::Context::new(client).unwrap();
        assert!(super::read(&context).is_ok());
        drop(peer);
        assert!(super::read(&context).is_err());
    }
    #[test]
    fn cancellation_interrupts_permission_wait() {
        let result = super::request(
            futures_lite::future::pending::<ashpd::Result<()>>(),
            &|| false,
        );
        assert!(result.unwrap_err().to_string().contains("canceled"));
    }
    #[test]
    fn configurable_shortcuts() {
        assert_eq!(super::chord("Ctrl+PageDown").unwrap(), [29, 109]);
        assert_eq!(super::chord("z").unwrap(), [44]);
        assert_eq!(super::chord("Space").unwrap(), [57]);
        assert!(super::chord("Space+z").is_err());
        assert!(super::chord("Ctrl+Ctrl+z").is_err());
        assert!(super::chord("").is_err());
    }
}
