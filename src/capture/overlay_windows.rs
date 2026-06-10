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
    InitCommonControls, TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_ADJUSTRECT,
    TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE, TTM_TRACKPOSITION, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP,
    TTS_NOPREFIX, TTTOOLINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetCursorPos, GetWindowRect, IsWindow,
    PeekMessageW, SendMessageW, TranslateMessage, MSG, PM_REMOVE, WS_EX_TOPMOST, WS_POPUP,
};

const BAR_COUNT: usize = 10;
const CURSOR_OFFSET: i32 = 16;
const NOTICE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const RECORDING_POLL_INTERVAL: Duration = Duration::from_millis(2);
const MAX_TOOLTIP_WIDTH: i32 = 420;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPrimary {
    Hidden,
    Recording,
    Transcribing,
    Typing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct OverlayHandle {
    tx: Sender<OverlayCommand>,
    latest_level: Arc<AtomicU32>,
}

#[derive(Debug, Clone)]
enum OverlayCommand {
    StartRecording(isize),
    SetPrimary(OverlayPrimary),
    Notify {
        level: NoticeLevel,
        text: String,
        duration: Option<Duration>,
    },
    ClearNotice,
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

    /// Starts a fresh dictation session. Any stale notice from a previous
    /// session is removed so the recording visualizer remains easy to read.
    pub fn start_recording(&self, target_window: isize) {
        let _ = self.tx.send(OverlayCommand::StartRecording(target_window));
    }

    /// Backwards-compatible alias used by the screenshot and latency helpers.
    pub fn show(&self, target_window: isize) {
        self.start_recording(target_window);
    }

    pub fn set_primary(&self, primary: OverlayPrimary) {
        let _ = self.tx.send(OverlayCommand::SetPrimary(primary));
    }

    pub fn notify_info(&self, text: impl Into<String>, duration: Option<Duration>) {
        self.notify(NoticeLevel::Info, text, duration);
    }

    pub fn notify_warning(&self, text: impl Into<String>, duration: Duration) {
        self.notify(NoticeLevel::Warning, text, Some(duration));
    }

    pub fn notify_error(&self, text: impl Into<String>, duration: Duration) {
        self.notify(NoticeLevel::Error, text, Some(duration));
    }

    pub fn clear_notice(&self) {
        let _ = self.tx.send(OverlayCommand::ClearNotice);
    }

    fn notify(&self, level: NoticeLevel, text: impl Into<String>, duration: Option<Duration>) {
        let _ = self.tx.send(OverlayCommand::Notify {
            level,
            text: text.into(),
            duration,
        });
    }

    pub fn set_level(&self, level: f32) {
        self.latest_level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn level_cell(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.latest_level)
    }

    /// Immediately removes both the primary state and any transient notice.
    pub fn hide(&self) {
        let _ = self.tx.send(OverlayCommand::Hide);
    }

    pub fn benchmark_latency(&self, iterations: usize) -> Vec<Duration> {
        let mut values = Vec::with_capacity(iterations);
        for index in 0..iterations {
            self.set_level((index % 2) as f32);
            let (tx, rx) = crossbeam_channel::bounded(1);
            if self
                .tx
                .send(OverlayCommand::Probe(Instant::now(), tx))
                .is_err()
            {
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
        let message = if state.should_render() {
            rx.recv_timeout(state.poll_interval())
        } else {
            rx.recv()
                .map_err(|_| crossbeam_channel::RecvTimeoutError::Disconnected)
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

#[derive(Debug, Clone)]
struct Notice {
    level: NoticeLevel,
    text: String,
    expires_at: Option<Instant>,
}

impl Notice {
    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| Instant::now() >= expires_at)
            .unwrap_or(false)
    }
}

struct TooltipState {
    hwnd: HWND,
    owner_hwnd: HWND,
    tool: TTTOOLINFOW,
    primary: OverlayPrimary,
    notice: Option<Notice>,
    target_level: f32,
    display_level: f32,
    animation_phase: usize,
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
            primary: OverlayPrimary::Hidden,
            notice: None,
            target_level: 0.0,
            display_level: 0.0,
            animation_phase: 0,
            last_text: String::new(),
            text_buf: Vec::new(),
        }
    }

    fn start_recording(&mut self, _target_window: isize) {
        self.notice = None;
        self.primary = OverlayPrimary::Recording;
        self.target_level = 0.0;
        self.display_level = 0.0;
        self.animation_phase = 0;
    }

    fn set_primary(&mut self, primary: OverlayPrimary) {
        self.primary = primary;
        if primary != OverlayPrimary::Recording {
            self.target_level = 0.0;
            self.display_level = 0.0;
        }
    }

    fn notify(&mut self, level: NoticeLevel, text: String, duration: Option<Duration>) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if let Some(current) = self.notice.as_ref().filter(|notice| !notice.is_expired()) {
            // Do not turn repeated callbacks into an endlessly extended notice.
            if current.level == level && current.text == text {
                return;
            }
            // A lower-priority message should never cover an active warning or error.
            if current.level > level {
                return;
            }
        }
        self.notice = Some(Notice {
            level,
            text,
            expires_at: duration.map(|duration| Instant::now() + duration),
        });
    }

    fn handle_command(&mut self, command: OverlayCommand) {
        match command {
            OverlayCommand::StartRecording(hwnd) => self.start_recording(hwnd),
            OverlayCommand::SetPrimary(primary) => self.set_primary(primary),
            OverlayCommand::Notify {
                level,
                text,
                duration,
            } => self.notify(level, text, duration),
            OverlayCommand::ClearNotice => self.notice = None,
            OverlayCommand::Hide => self.hide(),
            OverlayCommand::Probe(sent_at, tx) => {
                let _ = tx.try_send(sent_at.elapsed());
            }
        }
    }

    fn set_level(&mut self, level: f32) {
        if self.primary == OverlayPrimary::Recording {
            self.target_level = level.clamp(0.0, 1.0);
        }
    }

    fn should_render(&self) -> bool {
        self.primary != OverlayPrimary::Hidden || self.notice.is_some()
    }

    fn poll_interval(&self) -> Duration {
        if self.primary == OverlayPrimary::Recording {
            RECORDING_POLL_INTERVAL
        } else {
            NOTICE_POLL_INTERVAL
        }
    }

    fn hide(&mut self) {
        self.primary = OverlayPrimary::Hidden;
        self.notice = None;
        self.target_level = 0.0;
        self.display_level = 0.0;
        self.destroy_tooltip();
    }

    fn tick(&mut self) {
        if self.notice.as_ref().is_some_and(Notice::is_expired) {
            self.notice = None;
        }
        if !self.should_render() {
            self.destroy_tooltip();
            return;
        }
        if let Err(error) = self.ensure_tooltip() {
            tracing::warn!(%error, "native tooltip overlay failed to open");
            self.primary = OverlayPrimary::Hidden;
            self.notice = None;
            return;
        }

        if self.primary == OverlayPrimary::Recording {
            self.display_level = self.display_level * 0.10 + self.target_level * 0.90;
            self.animation_phase = self.animation_phase.wrapping_add(1);
        }
        let text = self.render_text();
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

    fn render_text(&self) -> String {
        let primary = match self.primary {
            OverlayPrimary::Hidden => None,
            OverlayPrimary::Recording => {
                Some(ascii_visualizer(self.display_level, self.animation_phase))
            }
            OverlayPrimary::Transcribing => Some("Transcribing...".to_owned()),
            OverlayPrimary::Typing => Some("Typing...".to_owned()),
        };
        match (primary, self.notice.as_ref()) {
            (Some(primary), Some(notice)) => format!("{primary}\r\n{}", notice.text),
            (Some(primary), None) => primary,
            (None, Some(notice)) => notice.text.clone(),
            (None, None) => String::new(),
        }
    }

    fn ensure_tooltip(&mut self) -> Result<()> {
        if !self.hwnd.is_null() && unsafe { IsWindow(self.hwnd) != 0 } {
            return Ok(());
        }
        self.destroy_tooltip();
        self.text_buf = "\0".encode_utf16().collect();
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
            self.destroy_tooltip();
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
            self.destroy_tooltip();
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
                self.destroy_tooltip();
                return Err(anyhow!("TTM_ADDTOOLW failed"));
            }
        }
        tracing::debug!("native tooltip overlay opened");
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
            let width = (text_rect.right - text_rect.left).clamp(160, MAX_TOOLTIP_WIDTH);
            SendMessageW(self.hwnd, TTM_SETMAXTIPWIDTH, 0, width as LPARAM);
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
        (
            (rect.right - rect.left).max(32),
            (rect.bottom - rect.top).max(20),
        )
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

fn ascii_visualizer(level: f32, _phase: usize) -> String {
    const GLYPHS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
    let level = level.clamp(0.0, 1.0);
    let center = (BAR_COUNT as f32 - 1.0) * 0.5;
    let mut line = String::with_capacity(BAR_COUNT * 3);
    for idx in 0..BAR_COUNT {
        let distance = (idx as f32 - center).abs() / center.max(1.0);
        let envelope = (1.0 - distance * 0.88).max(0.12);
        let strength = (level * envelope + 0.04).clamp(0.0, 1.0);
        let glyph = GLYPHS[(strength * (GLYPHS.len() - 1) as f32).round() as usize];
        line.push(glyph);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_waveform_is_prefix_free_unicode_and_stationary() {
        let first = ascii_visualizer(0.4, 0);
        let next_phase = ascii_visualizer(0.4, 1);
        let louder = ascii_visualizer(0.8, 0);
        assert!(!first.to_ascii_lowercase().contains("rec"));
        assert_eq!(first.chars().count(), BAR_COUNT);
        assert_eq!(first, next_phase);
        assert_ne!(first, louder);
        assert!(first.chars().all(|glyph| "▁▂▃▄▅▆▇".contains(glyph)));
    }
}
