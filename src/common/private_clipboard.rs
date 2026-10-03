//! An X11 selection owner which offers the KDE/clipboard-manager exclusion hint
//! atomically with text. Payloads stay in memory, never argv or temporary files.
use anyhow::{bail, Result};
use std::io::Read;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, Property,
    SelectionNotifyEvent, WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

const MAX_PAYLOAD: usize = 1024 * 1024;
const CHUNK: usize = 32 * 1024;
struct Transfer {
    window: u32,
    property: Atom,
    target: Atom,
    bytes: Vec<u8>,
    offset: usize,
    touched: Instant,
}

pub fn serve_x11(primary: bool) -> Result<()> {
    let mut text = String::new();
    std::io::stdin()
        .take(MAX_PAYLOAD as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > MAX_PAYLOAD {
        bail!("clipboard payload exceeds limit");
    }
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let window = connection.generate_id()?;
    connection
        .create_window(
            0,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?
        .check()?;
    let atom = |name: &str| -> Result<Atom> {
        Ok(connection
            .intern_atom(false, name.as_bytes())?
            .reply()?
            .atom)
    };
    let selection = atom(if primary { "PRIMARY" } else { "CLIPBOARD" })?;
    let targets = atom("TARGETS")?;
    let incr = atom("INCR")?;
    let hint = atom("x-kde-passwordManagerHint")?;
    let utf8 = atom("UTF8_STRING")?;
    let text_atom = atom("TEXT")?;
    let string = u32::from(AtomEnum::STRING);
    let plain = atom("text/plain")?;
    let plain_utf8 = atom("text/plain;charset=utf-8")?;
    let supported = [targets, utf8, text_atom, string, plain, plain_utf8, hint];
    connection
        .set_selection_owner(window, selection, x11rb::CURRENT_TIME)?
        .check()?;
    connection.flush()?;
    if connection.get_selection_owner(selection)?.reply()?.owner != window {
        bail!("clipboard ownership failed");
    }
    let mut transfers: Vec<Transfer> = Vec::new();
    loop {
        transfers.retain(|t| t.touched.elapsed() < Duration::from_secs(5));
        let Some(event) = connection.poll_for_event()? else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        match event {
            Event::SelectionClear(event) if event.selection == selection => return Ok(()),
            Event::SelectionRequest(request) => {
                let property = if request.property == 0 {
                    request.target
                } else {
                    request.property
                };
                let mut accepted = true;
                if request.target == targets {
                    connection
                        .change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            property,
                            AtomEnum::ATOM,
                            &supported,
                        )?
                        .check()?;
                } else if supported.contains(&request.target) {
                    let bytes = if request.target == hint {
                        b"secret".to_vec()
                    } else if request.target == string {
                        text.chars()
                            .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
                            .collect()
                    } else {
                        text.as_bytes().to_vec()
                    };
                    if bytes.len() <= CHUNK {
                        connection
                            .change_property8(
                                PropMode::REPLACE,
                                request.requestor,
                                property,
                                request.target,
                                &bytes,
                            )?
                            .check()?;
                    } else if transfers.len() < 8 {
                        connection
                            .change_window_attributes(
                                request.requestor,
                                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                                    .event_mask(EventMask::PROPERTY_CHANGE),
                            )?
                            .check()?;
                        connection
                            .change_property32(
                                PropMode::REPLACE,
                                request.requestor,
                                property,
                                incr,
                                &[bytes.len() as u32],
                            )?
                            .check()?;
                        transfers.push(Transfer {
                            window: request.requestor,
                            property,
                            target: request.target,
                            bytes,
                            offset: 0,
                            touched: Instant::now(),
                        });
                    } else {
                        accepted = false;
                    }
                } else {
                    accepted = false;
                }
                let reply = SelectionNotifyEvent {
                    response_type: SELECTION_NOTIFY_EVENT,
                    sequence: 0,
                    time: request.time,
                    requestor: request.requestor,
                    selection: request.selection,
                    target: request.target,
                    property: if accepted { property } else { 0 },
                };
                // Requestors can close while a transfer is in flight.
                let _ = connection
                    .send_event(false, request.requestor, EventMask::NO_EVENT, reply)?
                    .check();
                connection.flush()?;
            }
            Event::PropertyNotify(event) if event.state == Property::DELETE => {
                if let Some(index) = transfers
                    .iter()
                    .position(|t| t.window == event.window && t.property == event.atom)
                {
                    let transfer = &mut transfers[index];
                    let end = (transfer.offset + CHUNK).min(transfer.bytes.len());
                    let result = connection
                        .change_property8(
                            PropMode::REPLACE,
                            transfer.window,
                            transfer.property,
                            transfer.target,
                            &transfer.bytes[transfer.offset..end],
                        )?
                        .check();
                    if result.is_err() || transfer.offset == transfer.bytes.len() {
                        transfers.swap_remove(index);
                    } else {
                        transfer.offset = end;
                        transfer.touched = Instant::now();
                    }
                    connection.flush()?;
                }
            }
            _ => {}
        }
    }
}
