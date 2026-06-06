use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use procmod_overlay::{Color, Overlay, OverlayTarget};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

const PILL_W: f32 = 280.0;
const PILL_H: f32 = 58.0;
const BOTTOM_MARGIN: f32 = 72.0;
const BAR_COUNT: usize = 13;

#[derive(Debug, Clone)]
pub struct OverlayHandle {
    tx: Sender<OverlayCommand>,
}

#[derive(Debug, Clone, Copy)]
enum OverlayCommand {
    Show(isize),
    Level(f32),
    Hide,
}

impl OverlayHandle {
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = unbounded::<OverlayCommand>();
        thread::spawn(move || overlay_thread(rx));
        Ok(Self { tx })
    }

    pub fn show(&self, target_window: isize) {
        let _ = self.tx.send(OverlayCommand::Show(target_window));
    }

    pub fn set_level(&self, level: f32) {
        let _ = self.tx.send(OverlayCommand::Level(level));
    }

    pub fn hide(&self) {
        let _ = self.tx.send(OverlayCommand::Hide);
    }
}

fn overlay_thread(rx: Receiver<OverlayCommand>) {
    let mut state = OverlayState::default();
    loop {
        match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(OverlayCommand::Show(hwnd)) => state.show(hwnd),
            Ok(OverlayCommand::Level(level)) => state.set_level(level),
            Ok(OverlayCommand::Hide) => state.hide(),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
        state.draw_frame();
    }
}

#[derive(Default)]
struct OverlayState {
    overlay: Option<Overlay>,
    target_window: isize,
    target_level: f32,
    display_level: f32,
}

impl OverlayState {
    fn show(&mut self, target_window: isize) {
        self.hide();
        self.target_window = target_window;
        self.target_level = 0.0;
        self.display_level = 0.0;
        match Overlay::new(OverlayTarget::Hwnd(target_window)) {
            Ok(overlay) => {
                tracing::info!(target_window, "recording overlay opened");
                self.overlay = Some(overlay);
            }
            Err(error) => tracing::warn!(%error, target_window, "recording overlay failed to open"),
        }
    }

    fn set_level(&mut self, level: f32) {
        self.target_level = level.clamp(0.0, 1.0);
    }

    fn hide(&mut self) {
        if self.overlay.is_some() {
            tracing::debug!("recording overlay closed");
        }
        self.overlay = None;
        self.target_window = 0;
        self.target_level = 0.0;
        self.display_level = 0.0;
    }

    fn draw_frame(&mut self) {
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        let Some((target_w, target_h)) = target_size(self.target_window) else {
            self.hide();
            return;
        };

        self.display_level = self.display_level * 0.35 + self.target_level * 0.65;
        if let Err(error) = draw_visualizer(overlay, target_w, target_h, self.display_level) {
            tracing::warn!(%error, "recording overlay draw failed");
            self.hide();
        }
    }
}

fn draw_visualizer(overlay: &mut Overlay, target_w: f32, target_h: f32, level: f32) -> Result<()> {
    overlay.begin_frame()?;
    let x = ((target_w - PILL_W) * 0.5).max(24.0);
    let y = (target_h - PILL_H - BOTTOM_MARGIN).max(24.0);

    draw_pill(
        overlay,
        x,
        y,
        PILL_W,
        PILL_H,
        PILL_H * 0.5,
        Color::rgba(10, 15, 18, 46),
    );

    let bars_w = 10.0;
    let gap = 7.0;
    let total_w = BAR_COUNT as f32 * bars_w + (BAR_COUNT - 1) as f32 * gap;
    let start_x = x + (PILL_W - total_w) * 0.5;
    let center_y = y + PILL_H * 0.5;
    for idx in 0..BAR_COUNT {
        let phase = ((idx as f32 / (BAR_COUNT - 1) as f32) - 0.5).abs();
        let height_bias = 1.0 - phase * 0.55;
        let h = 14.0 + level * 34.0 * height_bias;
        let bx = start_x + idx as f32 * (bars_w + gap);
        let by = center_y - h * 0.5;
        let color = if level > 0.04 {
            Color::rgba(73, 226, 198, 218)
        } else {
            Color::rgba(49, 149, 134, 104)
        };
        draw_pill(overlay, bx, by, bars_w, h, bars_w * 0.5, color);
    }
    overlay.end_frame()?;
    Ok(())
}

fn draw_pill(overlay: &mut Overlay, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
    let radius = radius.min(w * 0.5).min(h * 0.5);
    let rows = h.ceil() as i32;
    for row in 0..rows {
        let py = y + row as f32;
        let sample_y = py + 0.5;
        let inset = if sample_y < y + radius {
            let dy = y + radius - sample_y;
            radius - (radius * radius - dy * dy).max(0.0).sqrt()
        } else if sample_y > y + h - radius {
            let dy = sample_y - (y + h - radius);
            radius - (radius * radius - dy * dy).max(0.0).sqrt()
        } else {
            0.0
        };
        let rx = x + inset;
        let rw = (w - inset * 2.0).max(0.0);
        overlay.rect_filled(rx, py, rw, 1.0, color);
    }
}

fn target_size(target_window: isize) -> Option<(f32, f32)> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let ok = unsafe { GetWindowRect(target_window as HWND, &mut rect) };
    if ok == 0 {
        return None;
    }
    let w = (rect.right - rect.left) as f32;
    let h = (rect.bottom - rect.top) as f32;
    (w > 0.0 && h > 0.0).then_some((w, h))
}
