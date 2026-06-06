use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::Controls::{
    InitCommonControls, TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW,
    TTM_ADJUSTRECT, TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE, TTM_TRACKPOSITION,
    TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetCursorPos, GetWindowRect, IsWindow,
    PeekMessageW, SendMessageW, TranslateMessage, MSG, PM_REMOVE, WS_EX_TOPMOST, WS_POPUP,
};

const BAR_COUNT: usize = 14;
const MAX_LEVEL: usize = 8;
const CURSOR_OFFSET: i32 = 16;

#[derive(Debug, Clone)]
pub struct OverlayHandle {
    tx: Sender<OverlayCommand>,
    latest_level: Arc<AtomicU32>,
}

#[derive(Debug, Clone)]
enum OverlayCommand {
    Show(isize),
    Hide,
    Probe(Instant, Sender<Duration>),
}

impl OverlayHandle {
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = unbounded::<OverlayCommand>();
        let latest_level = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        thread::spawn({
            let latest_level = Arc::clone(&latest_level);
            move || overlay_thread(rx, latest_level)
        });
        Ok(Self { tx, latest_level })
    }

    pub fn show(&self, target_window: isize) {
        let _ = self.tx.send(OverlayCommand::Show(target_window));
    }

    pub fn set_level(&self, level: f32) {
        self.latest_level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn level_cell(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.latest_level)
    }

    pub fn hide(&self) {
        let _ = self.tx.send(OverlayCommand::Hide);
    }

    pub fn benchmark_latency(&self, iterations: usize) -> Vec<Duration> {
        let mut values = Vec::with_capacity(iterations);
        for index in 0..iterations {
            self.set_level((index % 2) as f32);
            let (tx, rx) = crossbeam_channel::bounded(1);
            if self.tx.send(OverlayCommand::Probe(Instant::now(), tx)).is_err() {
                break;
            }
            if let Ok(value) = rx.recv_timeout(Duration::from_secs(1)) {
                values.push(value);
            }
        }
        values
    }
}

fn overlay_thread(rx: Receiver<OverlayCommand>, latest_level: Arc<AtomicU32>) {
    let mut state = TooltipState::new();
    loop {
        let message = if state.is_visible() {
            rx.recv_timeout(Duration::from_millis(2))
        } else {
            rx.recv().map_err(|_| crossbeam_channel::RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(command) => {
                state.handle_command(command);
                while let Ok(command) = rx.try_recv() {
                    state.handle_command(command);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
        pump_messages();
        state.set_level(f32::from_bits(latest_level.load(Ordering::Relaxed)));
        state.tick();
    }
}

fn pump_messages() {
    unsafe {
        let mut msg: MSG = zeroed();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct TooltipState {
    hwnd: HWND,
    owner_hwnd: HWND,
    tool: TTTOOLINFOW,
    visible: bool,
    target_window: isize,
    target_level: f32,
    display_level: f32,
    last_text: String,
    text_buf: Vec<u16>,
}

impl TooltipState {
    fn new() -> Self {
        unsafe {
            InitCommonControls();
        }
        Self {
            hwnd: null_mut(),
            owner_hwnd: null_mut(),
            tool: unsafe { zeroed() },
            visible: false,
            target_window: 0,
            target_level: 0.0,
            display_level: 0.0,
            last_text: String::new(),
            text_buf: Vec::new(),
        }
    }

    fn show(&mut self, target_window: isize) {
        self.target_window = target_window;
        self.target_level = 0.0;
        self.display_level = 0.0;
        self.visible = true;
        if let Err(error) = self.ensure_tooltip() {
            tracing::warn!(%error, "native tooltip visualizer failed to open");
            self.visible = false;
        }
    }

    fn handle_command(&mut self, command: OverlayCommand) {
        match command {
            OverlayCommand::Show(hwnd) => self.show(hwnd),
            OverlayCommand::Hide => self.hide(),
            OverlayCommand::Probe(sent_at, tx) => {
                let _ = tx.try_send(sent_at.elapsed());
            }
        }
    }

    fn set_level(&mut self, level: f32) {
        if self.visible {
            self.target_level = level.clamp(0.0, 1.0);
        }
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.target_level = 0.0;
        self.display_level = 0.0;
        self.destroy_tooltip();
    }

    fn tick(&mut self) {
        if !self.visible {
            return;
        }
        if let Err(error) = self.ensure_tooltip() {
            tracing::warn!(%error, "native tooltip visualizer failed to open");
            self.visible = false;
            return;
        }

        self.display_level = self.display_level * 0.10 + self.target_level * 0.90;
        let text = ascii_visualizer(self.display_level);
        let newly_created = self.last_text.is_empty();
        if text != self.last_text {
            self.set_text(&text);
            self.last_text = text;
        }

        let mut cursor = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut cursor);
        }
        let mut point = POINT {
            x: cursor.x + CURSOR_OFFSET,
            y: cursor.y + CURSOR_OFFSET,
        };
        let work_area = monitor_work_area(point);

        self.update_max_width(work_area);
        unsafe {
            if newly_created {
                SendMessageW(
                    self.hwnd,
                    TTM_TRACKPOSITION,
                    0,
                    make_lparam(point.x, point.y),
                );
                SendMessageW(
                    self.hwnd,
                    TTM_TRACKACTIVATE,
                    1,
                    &self.tool as *const TTTOOLINFOW as LPARAM,
                );
            }
        }

        let size = self.tooltip_size();
        if point.x + size.0 >= work_area.right {
            point.x = work_area.right - size.0 - 1;
        }
        if point.y + size.1 >= work_area.bottom {
            point.y = work_area.bottom - size.1 - 1;
        }
        if cursor_inside(point, size, cursor) {
            point.x = cursor.x - size.0 - 3;
            point.y = cursor.y - size.1 - 3;
        }

        unsafe {
            SendMessageW(
                self.hwnd,
                TTM_TRACKPOSITION,
                0,
                make_lparam(point.x, point.y),
            );
            SendMessageW(
                self.hwnd,
                TTM_TRACKACTIVATE,
                1,
                &self.tool as *const TTTOOLINFOW as LPARAM,
            );
        }
    }

    fn ensure_tooltip(&mut self) -> Result<()> {
        if !self.hwnd.is_null() && unsafe { IsWindow(self.hwnd) != 0 } {
            return Ok(());
        }
        self.destroy_tooltip();
        self.set_text(&ascii_visualizer(0.0));
        self.owner_hwnd = unsafe {
            CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("UvoxTooltipOwner"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if self.owner_hwnd.is_null() {
            return Err(anyhow!("CreateWindowExW tooltip owner failed"));
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                null_mut(),
                WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(anyhow!("CreateWindowExW TOOLTIPS_CLASSW failed"));
        }
        self.hwnd = hwnd;
        self.tool = TTTOOLINFOW {
            cbSize: tooltip_info_size(),
            uFlags: TTF_TRACK | TTF_ABSOLUTE,
            hwnd: self.owner_hwnd,
            uId: 1,
            rect: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            hinst: null_mut(),
            lpszText: self.text_buf.as_mut_ptr(),
            lParam: 0,
            lpReserved: null_mut(),
        };
        unsafe {
            let added = SendMessageW(
                self.hwnd,
                TTM_ADDTOOLW,
                0,
                &self.tool as *const TTTOOLINFOW as LPARAM,
            );
            if added == 0 {
                return Err(anyhow!("TTM_ADDTOOLW failed"));
            }
        }
        tracing::debug!("native tooltip visualizer opened");
        Ok(())
    }

    fn set_text(&mut self, text: &str) {
        self.text_buf = text.encode_utf16().chain(Some(0)).collect();
        self.tool.lpszText = self.text_buf.as_mut_ptr();
        if !self.hwnd.is_null() {
            unsafe {
                SendMessageW(
                    self.hwnd,
                    TTM_UPDATETIPTEXTW,
                    0,
                    &self.tool as *const TTTOOLINFOW as LPARAM,
                );
            }
        }
    }

    fn update_max_width(&self, work_area: RECT) {
        let mut text_rect = work_area;
        unsafe {
            SendMessageW(
                self.hwnd,
                TTM_ADJUSTRECT,
                0,
                &mut text_rect as *mut RECT as LPARAM,
            );
            SendMessageW(
                self.hwnd,
                TTM_SETMAXTIPWIDTH,
                0,
                (text_rect.right - text_rect.left) as LPARAM,
            );
        }
    }

    fn tooltip_size(&self) -> (i32, i32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 160,
            bottom: 32,
        };
        unsafe {
            GetWindowRect(self.hwnd, &mut rect);
        }
        ((rect.right - rect.left).max(32), (rect.bottom - rect.top).max(20))
    }

    fn destroy_tooltip(&mut self) {
        if !self.hwnd.is_null() {
            unsafe {
                DestroyWindow(self.hwnd);
            }
            self.hwnd = null_mut();
            self.last_text.clear();
        }
        if !self.owner_hwnd.is_null() {
            unsafe {
                DestroyWindow(self.owner_hwnd);
            }
            self.owner_hwnd = null_mut();
        }
    }
}

impl Drop for TooltipState {
    fn drop(&mut self) {
        self.destroy_tooltip();
    }
}

fn ascii_visualizer(level: f32) -> String {
    let level = level.clamp(0.0, 1.0);
    let active = (level * MAX_LEVEL as f32).round() as usize;
    let center = (BAR_COUNT as f32 - 1.0) * 0.5;
    let mut line = String::with_capacity(BAR_COUNT + 10);
    line.push_str("rec ");
    for idx in 0..BAR_COUNT {
        let distance = (idx as f32 - center).abs();
        let height = ((MAX_LEVEL as f32 - distance * 0.85).round() as isize).max(1) as usize;
        line.push(if height <= active { '|' } else { '.' });
    }
    line
}

fn monitor_work_area(point: POINT) -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            return info.rcWork;
        }
    }
    RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

fn cursor_inside(point: POINT, size: (i32, i32), cursor: POINT) -> bool {
    cursor.x >= point.x
        && cursor.x <= point.x + size.0
        && cursor.y >= point.y
        && cursor.y <= point.y + size.1
}

fn make_lparam(x: i32, y: i32) -> LPARAM {
    ((y as u32) << 16 | (x as u32 & 0xffff)) as LPARAM
}

fn tooltip_info_size() -> u32 {
    (std::mem::size_of::<TTTOOLINFOW>() - std::mem::size_of::<*mut std::ffi::c_void>()) as u32
}
