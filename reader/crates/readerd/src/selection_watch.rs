//! Observe selection ownership, without requesting payloads or retaining history.
//! This connection exists only during a shortcut-copy request.
use anyhow::{Context, Result};
use std::os::fd::{AsFd, AsRawFd};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
    ext_data_control_offer_v1 as offer,
};
use x11rb::{connection::Connection as _, protocol::xfixes};

pub enum SelectionWatch {
    Wayland {
        connection: Connection,
        queue: EventQueue<State>,
        state: State,
    },
    X11 {
        connection: Box<x11rb::rust_connection::RustConnection>,
        window: u32,
        revision: u64,
    },
}

impl SelectionWatch {
    pub fn start() -> Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            let connection = Connection::connect_to_env()?;
            let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
            let handle = queue.handle();
            let manager: manager::ExtDataControlManagerV1 =
                globals.bind(&handle, 1..=1, ()).context(
                    "This compositor cannot observe clipboard changes; copy the selection manually",
                )?;
            let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=9, ())?;
            let _device = manager.get_data_device(&seat, &handle, ());
            let mut state = State::default();
            queue.roundtrip(&mut state)?;
            Ok(Self::Wayland {
                connection,
                queue,
                state,
            })
        } else {
            use x11rb::protocol::{xfixes::ConnectionExt as _, xproto::*};
            let (connection, screen) = x11rb::connect(None)?;
            connection.xfixes_query_version(5, 0)?.reply()?;
            let window = connection.generate_id()?;
            connection
                .create_window(
                    0,
                    window,
                    connection.setup().roots[screen].root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    WindowClass::INPUT_ONLY,
                    0,
                    &CreateWindowAux::new(),
                )?
                .check()?;
            let selection = connection.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
            connection
                .xfixes_select_selection_input(
                    window,
                    selection,
                    xfixes::SelectionEventMask::SET_SELECTION_OWNER
                        | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                        | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
                )?
                .check()?;
            connection.get_input_focus()?.reply()?;
            while connection.poll_for_event()?.is_some() {}
            Ok(Self::X11 {
                connection: Box::new(connection),
                window,
                revision: 0,
            })
        }
    }

    pub fn revision(&mut self) -> Result<u64> {
        match self {
            Self::Wayland {
                connection,
                queue,
                state,
            } => {
                queue.dispatch_pending(state)?;
                connection.flush()?;
                if let Some(read) = queue.prepare_read() {
                    let mut descriptor = libc::pollfd {
                        fd: connection.as_fd().as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // Nonblocking: cancellation is checked by the caller between probes.
                    if unsafe { libc::poll(&mut descriptor, 1, 0) } > 0 {
                        read.read()?;
                    }
                }
                queue.dispatch_pending(state)?;
                anyhow::ensure!(
                    !state.finished,
                    "clipboard observer was closed by the compositor"
                );
                Ok(state.revision)
            }
            Self::X11 {
                connection,
                window,
                revision,
            } => {
                while let Some(event) = connection.poll_for_event()? {
                    if matches!(event, x11rb::protocol::Event::XfixesSelectionNotify(event) if event.window == *window)
                    {
                        *revision = revision.wrapping_add(1);
                    }
                }
                Ok(*revision)
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    revision: u64,
    finished: bool,
    offers: Vec<offer::ExtDataControlOfferV1>,
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
        _: &mut Self,
        _: &offer::ExtDataControlOfferV1,
        _: offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
            device::Event::DataOffer { id } => state.offers.push(id),
            device::Event::Selection { .. } => {
                state.revision = state.revision.wrapping_add(1);
                for offer in state.offers.drain(..) {
                    offer.destroy();
                }
            }
            device::Event::Finished => state.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, device::ExtDataControlDeviceV1, [0 => (offer::ExtDataControlOfferV1, ())]);
}
