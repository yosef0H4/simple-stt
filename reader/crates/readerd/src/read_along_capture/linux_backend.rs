//! X11 read-along support. Wayland selection/capture is provided by the KDE
//! backend module when the session exposes the KWin screenshot interface.
use super::*;
use anyhow::Context;
use image::{ImageBuffer, ImageFormat, Rgb};
use std::{fs::OpenOptions, io::BufWriter, os::unix::fs::OpenOptionsExt, thread, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt, ImageOrder, Window, WindowClass},
        Event,
    },
    rust_connection::RustConnection,
};

pub(super) struct LinuxBackend {
    connection: RustConnection,
    root: Window,
    screen_num: usize,
}

impl LinuxBackend {
    pub(super) fn new() -> Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            bail!(
                "X11 read-along cannot address native Wayland windows; use the KDE window-capture backend"
            )
        }
        let (connection, screen_num) =
            x11rb::connect(None).context("connecting to the X11 display")?;
        let root = connection
            .setup()
            .roots
            .get(screen_num)
            .context("X11 screen is unavailable")?
            .root;
        Ok(Self {
            connection,
            root,
            screen_num,
        })
    }

    fn bounds(&self, window: Window) -> Result<Option<Rect>> {
        let attributes = match self.connection.get_window_attributes(window)?.reply() {
            Ok(attributes)
                if attributes.map_state == xproto::MapState::VIEWABLE
                    && attributes.class == WindowClass::INPUT_OUTPUT =>
            {
                attributes
            }
            _ => return Ok(None),
        };
        let _ = attributes;
        let geometry = match self.connection.get_geometry(window)?.reply() {
            Ok(geometry) if geometry.width > 0 && geometry.height > 0 => geometry,
            _ => return Ok(None),
        };
        let origin = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)?
            .reply()?;
        let hidden = self.atom("_NET_WM_STATE_HIDDEN")?;
        let state = self.atom("_NET_WM_STATE")?;
        let minimized = self
            .connection
            .get_property(false, window, state, xproto::AtomEnum::ATOM, 0, 128)?
            .reply()?;
        if minimized.type_ == u32::from(xproto::AtomEnum::ATOM)
            && minimized
                .value32()
                .is_some_and(|values| values.into_iter().any(|value| value == hidden))
        {
            return Ok(None);
        }
        Ok(Some(Rect {
            x: i32::from(origin.dst_x),
            y: i32::from(origin.dst_y),
            width: u32::from(geometry.width),
            height: u32::from(geometry.height),
        }))
    }

    fn atom(&self, name: &str) -> Result<xproto::Atom> {
        Ok(self
            .connection
            .intern_atom(false, name.as_bytes())?
            .reply()?
            .atom)
    }

    fn window_at(&self, x: i32, y: i32) -> Result<Option<Window>> {
        let stacking = self
            .connection
            .intern_atom(false, b"_NET_CLIENT_LIST_STACKING")?
            .reply()?
            .atom;
        let result = self
            .connection
            .get_property(
                false,
                self.root,
                stacking,
                xproto::AtomEnum::WINDOW,
                0,
                4096,
            )?
            .reply()?;
        if let Some(windows) = result.value32() {
            for window in windows.collect::<Vec<_>>().into_iter().rev() {
                if self.bounds(window)?.is_some_and(|b| {
                    b.contains(Rect {
                        x,
                        y,
                        width: 1,
                        height: 1,
                    })
                }) {
                    return Ok(Some(window));
                }
            }
        }
        Ok(None)
    }

    fn save_region(
        &self,
        bounds: Rect,
        region: Rect,
        current: &dyn Fn() -> bool,
    ) -> Result<PathBuf> {
        ensure!(current(), "read-along capture canceled");
        let screen = self
            .connection
            .setup()
            .roots
            .get(self.screen_num)
            .context("X11 screen is unavailable")?;
        let abs = region.at_window(bounds)?;
        let image_x =
            i16::try_from(abs.x).context("X11 screenshot x coordinate is out of range")?;
        let image_y =
            i16::try_from(abs.y).context("X11 screenshot y coordinate is out of range")?;
        let image_width =
            u16::try_from(abs.width).context("X11 screenshot width exceeds the protocol limit")?;
        let image_height = u16::try_from(abs.height)
            .context("X11 screenshot height exceeds the protocol limit")?;
        let pixels = self
            .connection
            .get_image(
                xproto::ImageFormat::Z_PIXMAP,
                self.root,
                image_x,
                image_y,
                image_width,
                image_height,
                u32::MAX,
            )?
            .reply()?
            .data;
        ensure!(current(), "read-along capture canceled");
        let format = self
            .connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == screen.root_depth)
            .context("X11 root pixel format is unavailable")?;
        let bpp = usize::from(format.bits_per_pixel);
        ensure!(
            bpp == 24 || bpp == 32,
            "unsupported X11 root pixel format ({bpp} bpp)"
        );
        let pad = usize::from(format.scanline_pad);
        let stride = (abs.width as usize * bpp).div_ceil(pad) * pad.div_ceil(8);
        let bytes_per_pixel = bpp / 8;
        let visual = self.connection.setup().roots[self.screen_num]
            .allowed_depths
            .iter()
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.visual_id == screen.root_visual)
            .context("X11 root visual is unavailable")?;
        let mut rgb = Vec::with_capacity(abs.width as usize * abs.height as usize * 3);
        for y in 0..abs.height as usize {
            for x in 0..abs.width as usize {
                let at = y * stride + x * bytes_per_pixel;
                ensure!(
                    at + bytes_per_pixel <= pixels.len(),
                    "truncated X11 screenshot data"
                );
                let value = match self.connection.setup().image_byte_order {
                    ImageOrder::LSB_FIRST => pixels[at..at + bytes_per_pixel]
                        .iter()
                        .enumerate()
                        .fold(0u32, |v, (i, b)| v | (u32::from(*b) << (i * 8))),
                    _ => pixels[at..at + bytes_per_pixel]
                        .iter()
                        .fold(0u32, |v, b| (v << 8) | u32::from(*b)),
                };
                rgb.extend_from_slice(&[
                    channel(value, visual.red_mask),
                    channel(value, visual.green_mask),
                    channel(value, visual.blue_mask),
                ]);
            }
        }
        let image: ImageBuffer<Rgb<u8>, _> =
            ImageBuffer::from_raw(abs.width, abs.height, rgb).context("building X11 screenshot")?;
        let path = private_capture_path()?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        if let Err(error) = image.write_to(&mut BufWriter::new(file), ImageFormat::Png) {
            let _ = std::fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(path)
    }
}

fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    (((pixel & mask) >> shift) * 255 / max) as u8
}

impl WindowBackend for LinuxBackend {
    fn choose_window_and_region(
        &mut self,
        current: &dyn Fn() -> bool,
    ) -> Result<(WindowSnapshot, Rect)> {
        ensure!(current(), "read-along selection canceled");
        let grab = self
            .connection
            .grab_pointer(
                false,
                self.root,
                xproto::EventMask::BUTTON_PRESS
                    | xproto::EventMask::BUTTON_RELEASE
                    | xproto::EventMask::POINTER_MOTION,
                xproto::GrabMode::ASYNC,
                xproto::GrabMode::ASYNC,
                0u32,
                0u32,
                x11rb::CURRENT_TIME,
            )?
            .reply()?;
        ensure!(
            grab.status == xproto::GrabStatus::SUCCESS,
            "could not grab the X11 pointer for region selection"
        );
        let result = (|| {
            let start = loop {
                ensure!(current(), "read-along selection canceled");
                if let Some(Event::ButtonPress(event)) = self.connection.poll_for_event()? {
                    break (i32::from(event.root_x), i32::from(event.root_y));
                }
                thread::sleep(Duration::from_millis(10));
            };
            let end = loop {
                ensure!(current(), "read-along selection canceled");
                if let Some(Event::ButtonRelease(event)) = self.connection.poll_for_event()? {
                    break (i32::from(event.root_x), i32::from(event.root_y));
                }
                thread::sleep(Duration::from_millis(10));
            };
            let region = Rect {
                x: start.0.min(end.0),
                y: start.1.min(end.1),
                width: start.0.abs_diff(end.0),
                height: start.1.abs_diff(end.1),
            };
            ensure!(
                region.width > 1 && region.height > 1,
                "read-along selection is too small"
            );
            let center = (
                i32::try_from(i64::from(region.x) + i64::from(region.width / 2))
                    .context("selected X11 region center x is out of range")?,
                i32::try_from(i64::from(region.y) + i64::from(region.height / 2))
                    .context("selected X11 region center y is out of range")?,
            );
            let window = self
                .window_at(center.0, center.1)?
                .context("selected region is not over a visible X11 window")?;
            let bounds = self
                .bounds(window)?
                .context("selected X11 window is minimized or destroyed")?;
            let snapshot = WindowSnapshot {
                id: WindowId::X11(window),
                bounds,
                minimized: false,
            };
            region.relative_to(bounds)?;
            Ok((snapshot, region))
        })();
        let _ = self.connection.ungrab_pointer(x11rb::CURRENT_TIME);
        result
    }

    fn inspect(&mut self, id: &WindowId) -> Result<Option<WindowSnapshot>> {
        let WindowId::X11(window) = id else {
            return Ok(None);
        };
        Ok(self.bounds(*window)?.map(|bounds| WindowSnapshot {
            id: id.clone(),
            bounds,
            minimized: false,
        }))
    }

    fn capture(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        current: &dyn Fn() -> bool,
    ) -> Result<PathBuf> {
        let WindowId::X11(id) = window.id else {
            bail!("X11 capture received a non-X11 window")
        };
        let live = self
            .bounds(id)?
            .context("selected window was closed or minimized")?;
        ensure!(
            live == window.bounds,
            "selected window moved; retry capture"
        );
        self.save_region(live, region, current)
    }

    fn input_ready(&mut self, window: &WindowSnapshot) -> Result<bool> {
        let WindowId::X11(target) = window.id else {
            return Ok(false);
        };
        let mut focused = self.connection.get_input_focus()?.reply()?.focus;
        for _ in 0..64 {
            if focused == target {
                return Ok(true);
            }
            if focused == self.root || focused == 0 {
                return Ok(false);
            }
            focused = self.connection.query_tree(focused)?.reply()?.parent;
        }
        Ok(false)
    }

    fn advance(
        &mut self,
        window: &WindowSnapshot,
        region: Rect,
        action: &AdvanceAction,
        current: &dyn Fn() -> bool,
    ) -> Result<()> {
        let WindowId::X11(id) = window.id else {
            bail!("X11 input received a non-X11 window")
        };
        let live = self
            .bounds(id)?
            .context("selected window was closed or minimized")?;
        ensure!(
            region.at_window(live).is_ok(),
            "saved region no longer fits the selected window"
        );
        ensure!(current(), "read-along action canceled");
        if !self.input_ready(window)? {
            return Err(super::FocusChanged.into());
        }
        let input = match action {
            AdvanceAction::Key(key) => crate::portal_input::Action::Key { key: key.clone() },
            AdvanceAction::Sequence(sequence) => crate::portal_input::Action::Sequence {
                sequence: sequence.clone(),
            },
            AdvanceAction::Click { x, y } => {
                ensure!(
                    *x < region.width && *y < region.height,
                    "read-along click is outside the selected region"
                );
                let point = region.at_window(live)?;
                crate::portal_input::Action::Click {
                    x: i32::try_from(i64::from(point.x) + i64::from(*x))?,
                    y: i32::try_from(i64::from(point.y) + i64::from(*y))?,
                }
            }
        };
        crate::portal_input::run(input, current)?;
        self.connection.flush()?;
        Ok(())
    }

    fn supports_window_bound_input(&self) -> bool {
        true
    }
}
