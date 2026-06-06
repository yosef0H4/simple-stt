use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Sender};
use std::mem::zeroed;
use std::ptr::null_mut;
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetMonitorInfoW,
    GetStockObject, InvalidateRect, MonitorFromWindow, RoundRect, SelectObject, SetBkMode,
    SetTextColor, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, TRANSPARENT,
    WHITE_BRUSH,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, MSG, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOWNA, WM_APP, WM_PAINT, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::winutil::{rgb, wide_null};

const CLASS: &str = "UvoxRecordingOverlay";
const WIDTH: i32 = 220;
const HEIGHT: i32 = 54;
const WM_OVERLAY_SHOW: u32 = WM_APP + 20;
const WM_OVERLAY_LEVEL: u32 = WM_APP + 21;
const WM_OVERLAY_HIDE: u32 = WM_APP + 22;

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
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        thread::spawn(move || unsafe {
            let hwnd = create_window();
            if hwnd.is_null() {
                let _ = ready_tx.send(Err(anyhow!("creating overlay window failed")));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            let rx_thread = rx.clone();
            let hwnd_value = hwnd as isize;
            thread::spawn(move || {
                while let Ok(command) = rx_thread.recv() {
                    let hwnd = hwnd_value as HWND;
                    match command {
                        OverlayCommand::Show(target) => {
                            let _ = PostMessageW(hwnd, WM_OVERLAY_SHOW, target as WPARAM, 0);
                        }
                        OverlayCommand::Level(level) => {
                            let scaled = (level.clamp(0.0, 1.0) * 1000.0) as WPARAM;
                            let _ = PostMessageW(hwnd, WM_OVERLAY_LEVEL, scaled, 0);
                        }
                        OverlayCommand::Hide => {
                            let _ = PostMessageW(hwnd, WM_OVERLAY_HIDE, 0, 0);
                        }
                    }
                }
            });
            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        });
        ready_rx.recv()??;
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

static mut LEVEL: f32 = 0.0;

unsafe fn create_window() -> HWND {
    let class = wide_null(CLASS);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: null_mut(),
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: GetStockObject(WHITE_BRUSH) as HBRUSH,
        lpszMenuName: null_mut(),
        lpszClassName: class.as_ptr(),
        cbClsExtra: 0,
        cbWndExtra: 0,
    };
    RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        class.as_ptr(),
        wide_null("Uvox Recording").as_ptr(),
        WS_POPUP,
        0,
        0,
        WIDTH,
        HEIGHT,
        null_mut(),
        null_mut(),
        null_mut(),
        null_mut(),
    );
    if !hwnd.is_null() {
        SetLayeredWindowAttributes(hwnd, 0, 225, 0x00000002);
    }
    hwnd
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_OVERLAY_SHOW => {
            position_overlay(hwnd, wparam as HWND);
            ShowWindow(hwnd, SW_SHOWNA);
            0
        }
        WM_OVERLAY_LEVEL => {
            LEVEL = (wparam as f32 / 1000.0).clamp(0.0, 1.0);
            InvalidateRect(hwnd, null_mut(), 1);
            0
        }
        WM_OVERLAY_HIDE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn position_overlay(hwnd: HWND, target: HWND) {
    let monitor = MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(monitor, &mut info);
    let work = info.rcWork;
    let x = work.left + ((work.right - work.left) - WIDTH) / 2;
    let y = work.bottom - HEIGHT - 56;
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        WIDTH,
        HEIGHT,
        SWP_NOACTIVATE | SWP_NOSIZE,
    );
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let bg = CreateSolidBrush(rgb(24, 26, 30));
    let accent = CreateSolidBrush(rgb(80, 210, 180));
    let dim = CreateSolidBrush(rgb(45, 52, 58));
    let old = SelectObject(hdc, bg);
    RoundRect(hdc, 0, 0, WIDTH, HEIGHT, 16, 16);
    SelectObject(hdc, old);

    let level = LEVEL;
    for idx in 0..12 {
        let phase = ((idx as f32 / 12.0) - 0.5).abs();
        let bar = (8.0 + level * 30.0 * (1.0 - phase)).round() as i32;
        let x = 24 + idx * 14;
        let y = (HEIGHT - bar) / 2;
        let rect = RECT {
            left: x,
            top: y,
            right: x + 7,
            bottom: y + bar,
        };
        FillRect(hdc, &rect, if level > 0.03 { accent } else { dim });
    }
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, rgb(230, 235, 240));
    DeleteObject(bg as _);
    DeleteObject(accent as _);
    DeleteObject(dim as _);
    EndPaint(hwnd, &ps);
}
