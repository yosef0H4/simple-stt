use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_CAPITAL;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    CapsLockDown,
    CapsLockUp,
}

static EVENT_TX: OnceLock<Sender<HotkeyEvent>> = OnceLock::new();
static CAPSLOCK_HELD: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        let is_caps = info.vkCode == VK_CAPITAL as u32;
        let injected = info.flags & LLKHF_INJECTED != 0;
        if is_caps && !injected {
            let event = match wparam as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    if CAPSLOCK_HELD.swap(true, Ordering::SeqCst) {
                        None
                    } else {
                        Some(HotkeyEvent::CapsLockDown)
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    if CAPSLOCK_HELD.swap(false, Ordering::SeqCst) {
                        Some(HotkeyEvent::CapsLockUp)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(event) = event {
                if let Some(tx) = EVENT_TX.get() {
                    let _ = tx.send(event);
                }
            }
            return 1; // Suppress normal CapsLock toggle while Uvox is active.
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

pub fn spawn_capslock_hook(tx: Sender<HotkeyEvent>) -> Result<thread::JoinHandle<()>> {
    EVENT_TX
        .set(tx)
        .map_err(|_| anyhow!("CapsLock hook already initialized"))?;
    Ok(thread::spawn(move || unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), null_mut(), 0);
        if hook.is_null() {
            tracing::error!("SetWindowsHookExW failed; global CapsLock capture is unavailable");
            return;
        }
        tracing::info!("CapsLock push-to-talk hook installed");
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    }))
}
