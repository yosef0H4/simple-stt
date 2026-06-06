use anyhow::{bail, Result};
use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::transcript::TextSink;

const UVOX_EXTRA_INFO: usize = 0x5556_4f58; // "UVOX"

pub fn foreground_window_id() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

fn unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE | if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: UVOX_EXTRA_INFO,
            },
        },
    }
}

pub fn send_unicode_text(text: &str) -> Result<()> {
    let mut inputs = Vec::with_capacity(text.encode_utf16().count() * 2);
    for code_unit in text.encode_utf16() {
        inputs.push(unicode_input(code_unit, false));
        inputs.push(unicode_input(code_unit, true));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        bail!("SendInput inserted {sent}/{} events; the target may reject injected input or have a higher integrity level", inputs.len());
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct WindowsTextSink;

impl TextSink for WindowsTextSink {
    fn focused_window(&self) -> isize {
        foreground_window_id()
    }

    fn send_text(&self, text: &str) -> Result<()> {
        send_unicode_text(text)
    }
}
