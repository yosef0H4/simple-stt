use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, PostMessageW, RegisterClassW, SetForegroundWindow,
    TrackPopupMenu, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HMENU,
    IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_APP,
    WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_OVERLAPPED,
};

use crate::winutil::{loword, rgb, wide_null};

const CLASS: &str = "UvoxTrayWindow";
const TRAY_UID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_TRAY_SET_STATUS: u32 = WM_APP + 2;
const WM_TRAY_SHUTDOWN: u32 = WM_APP + 3;
const ID_SETTINGS: usize = 1001;
const ID_DISABLE: usize = 1002;
const ID_RELOAD: usize = 1003;
const ID_LOG: usize = 1004;
const ID_TEST: usize = 1005;
const ID_UNLOAD: usize = 1006;
const ID_EXIT: usize = 1007;

static COMMAND_TX: OnceLock<Sender<TrayCommand>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Ready,
    Recording,
    Transcribing,
    Disabled,
    Error,
}

impl TrayStatus {
    fn tip(self) -> &'static str {
        match self {
            Self::Ready => "Uvox - Ready",
            Self::Recording => "Uvox - Recording",
            Self::Transcribing => "Uvox - Transcribing",
            Self::Disabled => "Uvox - Hotkey disabled",
            Self::Error => "Uvox - Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenSettings,
    ToggleHotkey,
    ReloadConfig,
    OpenLog,
    UnloadModel,
    TestModel,
    Exit,
}

pub struct TrayHandle {
    hwnd: HWND,
}

unsafe impl Send for TrayHandle {}
unsafe impl Sync for TrayHandle {}

impl TrayHandle {
    pub fn spawn(tx: Sender<TrayCommand>) -> Result<Self> {
        COMMAND_TX
            .set(tx)
            .map_err(|_| anyhow!("tray command channel already initialized"))?;
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        thread::spawn(move || unsafe {
            let hwnd = create_window();
            if hwnd.is_null() {
                let _ = ready_tx.send(Err(anyhow!("creating tray window failed")));
                return;
            }
            let _ = ready_tx.send(Ok(hwnd as isize));
            add_icon(hwnd, TrayStatus::Ready);
            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            delete_icon(hwnd);
        });
        let hwnd = ready_rx.recv()?? as HWND;
        Ok(Self { hwnd })
    }

    pub fn set_status(&self, status: TrayStatus) {
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_TRAY_SET_STATUS, status as WPARAM, 0);
        }
    }

    pub fn shutdown(&self) {
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_TRAY_SHUTDOWN, 0, 0);
        }
    }
}

unsafe fn create_window() -> HWND {
    let class = wide_null(CLASS);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: null_mut(),
        hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
        hCursor: null_mut(),
        hbrBackground: CreateSolidBrush(rgb(255, 255, 255)),
        lpszMenuName: null_mut(),
        lpszClassName: class.as_ptr(),
        cbClsExtra: 0,
        cbWndExtra: 0,
    };
    RegisterClassW(&wc);
    CreateWindowExW(
        0,
        class.as_ptr(),
        wide_null("Uvox").as_ptr(),
        WS_OVERLAPPED,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        0,
        0,
        null_mut(),
        null_mut(),
        null_mut(),
        null_mut(),
    )
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAY => {
            show_menu(hwnd);
            0
        }
        WM_TRAY_SET_STATUS => {
            let status = match wparam {
                1 => TrayStatus::Recording,
                2 => TrayStatus::Transcribing,
                3 => TrayStatus::Disabled,
                4 => TrayStatus::Error,
                _ => TrayStatus::Ready,
            };
            modify_icon(hwnd, status);
            0
        }
        WM_TRAY_SHUTDOWN => {
            delete_icon(hwnd);
            windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            0
        }
        WM_COMMAND => {
            let command = match loword(wparam as usize) as usize {
                ID_SETTINGS => Some(TrayCommand::OpenSettings),
                ID_DISABLE => Some(TrayCommand::ToggleHotkey),
                ID_RELOAD => Some(TrayCommand::ReloadConfig),
                ID_LOG => Some(TrayCommand::OpenLog),
                ID_UNLOAD => Some(TrayCommand::UnloadModel),
                ID_TEST => Some(TrayCommand::TestModel),
                ID_EXIT => Some(TrayCommand::Exit),
                _ => None,
            };
            if let (Some(tx), Some(command)) = (COMMAND_TX.get(), command) {
                let _ = tx.send(command);
            }
            0
        }
        WM_DESTROY => {
            delete_icon(hwnd);
            windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn notify_data(hwnd: HWND, status: TrayStatus) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = zeroed();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
    let tip = wide_null(status.tip());
    for (idx, value) in tip.iter().copied().enumerate().take(data.szTip.len()) {
        data.szTip[idx] = value;
    }
    data
}

unsafe fn add_icon(hwnd: HWND, status: TrayStatus) {
    let mut data = notify_data(hwnd, status);
    Shell_NotifyIconW(NIM_ADD, &mut data);
}

unsafe fn modify_icon(hwnd: HWND, status: TrayStatus) {
    let mut data = notify_data(hwnd, status);
    Shell_NotifyIconW(NIM_MODIFY, &mut data);
}

unsafe fn delete_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = zeroed();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    Shell_NotifyIconW(NIM_DELETE, &mut data);
}

unsafe fn show_menu(hwnd: HWND) {
    let menu: HMENU = CreatePopupMenu();
    AppendMenuW(
        menu,
        MF_STRING,
        ID_SETTINGS,
        wide_null("Open Settings").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        ID_DISABLE,
        wide_null("Disable Hotkey").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        ID_RELOAD,
        wide_null("Reload Config").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        ID_UNLOAD,
        wide_null("Unload Model").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        ID_LOG,
        wide_null("Open Latest Log").as_ptr(),
    );
    AppendMenuW(menu, MF_STRING, ID_TEST, wide_null("Test Model").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
    AppendMenuW(menu, MF_STRING, ID_EXIT, wide_null("Exit").as_ptr());
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        point.x,
        point.y,
        0,
        hwnd,
        null_mut(),
    );
    DestroyMenu(menu);
}
