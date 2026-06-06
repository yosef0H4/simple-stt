use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{bounded, unbounded, Sender};
use slint::ComponentHandle;
use std::thread;

use crate::slint_ui::RecordingOverlay;

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
        let (ready_tx, ready_rx) = bounded(1);
        thread::spawn(move || {
            let app = match RecordingOverlay::new() {
                Ok(app) => app,
                Err(error) => {
                    let _ =
                        ready_tx.send(Err(anyhow!(error).context("creating recording overlay")));
                    return;
                }
            };
            app.set_level(0.0);
            if let Err(error) = app.show() {
                let _ = ready_tx.send(Err(anyhow!(error).context("showing recording overlay")));
                return;
            }
            app.hide().ok();
            let weak = app.as_weak();
            let _ = ready_tx.send(Ok(()));
            thread::spawn(move || {
                while let Ok(command) = rx.recv() {
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak.upgrade() {
                            match command {
                                OverlayCommand::Show(target_window) => {
                                    position_overlay(&app, target_window);
                                    if let Err(error) = app.show() {
                                        tracing::error!(%error, "showing Slint overlay failed");
                                    }
                                    apply_click_through(&app);
                                }
                                OverlayCommand::Level(level) => {
                                    app.set_level(level.clamp(0.0, 1.0));
                                }
                                OverlayCommand::Hide => {
                                    let _ = app.hide();
                                }
                            }
                        }
                    });
                }
            });
            if let Err(error) = slint::run_event_loop_until_quit() {
                tracing::error!(%error, "Slint overlay event loop failed");
            }
        });
        ready_rx.recv().context("waiting for overlay UI")??;
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

#[cfg(windows)]
fn position_overlay(app: &RecordingOverlay, target_window: isize) {
    use std::mem::zeroed;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    unsafe {
        let monitor = MonitorFromWindow(target_window as HWND, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            let width = 260_i32;
            let height = 64_i32;
            let work = info.rcWork;
            let x = work.left + ((work.right - work.left) - width) / 2;
            let y = work.bottom - height - 56;
            app.window()
                .set_position(slint::PhysicalPosition::new(x, y));
        }
    }
}

#[cfg(not(windows))]
fn position_overlay(_app: &RecordingOverlay, _target_window: isize) {}

#[cfg(windows)]
fn apply_click_through(app: &RecordingOverlay) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };

    let window_handle = app.window().window_handle();
    let handle = match window_handle.window_handle() {
        Ok(handle) => handle,
        Err(error) => {
            tracing::warn!(%error, "Slint overlay native window handle is unavailable");
            return;
        }
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        tracing::warn!("Slint overlay did not expose a Win32 window handle");
        return;
    };
    unsafe {
        let hwnd = handle.hwnd.get();
        let current = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
        let desired = current
            | WS_EX_LAYERED as isize
            | WS_EX_TRANSPARENT as isize
            | WS_EX_NOACTIVATE as isize
            | WS_EX_TOOLWINDOW as isize;
        if SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, desired) == 0 {
            tracing::warn!("setting Slint overlay click-through styles may have failed");
        }
    }
}

#[cfg(not(windows))]
fn apply_click_through(_app: &RecordingOverlay) {}
