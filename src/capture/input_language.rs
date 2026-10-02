use crate::config::{SpeechLanguage, SpeechLanguageMode};
use anyhow::{bail, Result};

pub fn resolve_speech_language(mode: SpeechLanguageMode) -> Result<SpeechLanguage> {
    match mode {
        SpeechLanguageMode::English => Ok(SpeechLanguage::English),
        SpeechLanguageMode::Arabic => Ok(SpeechLanguage::Arabic),
        SpeechLanguageMode::FollowKeyboard => foreground_input_language(),
    }
}

fn language_from_langid(lang_id: u16) -> Result<SpeechLanguage> {
    match lang_id & 0x03ff {
        0x01 => Ok(SpeechLanguage::Arabic),
        0x09 => Ok(SpeechLanguage::English),
        _ => bail!(
            "The active keyboard language is unsupported; select English or Arabic in Settings"
        ),
    }
}

#[cfg(windows)]
fn foreground_input_language() -> Result<SpeechLanguage> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            bail!("No foreground window; select English or Arabic in Settings");
        }
        let thread = GetWindowThreadProcessId(window, std::ptr::null_mut());
        if thread == 0 {
            bail!("Cannot read foreground keyboard language");
        }
        language_from_langid(GetKeyboardLayout(thread) as usize as u16)
    }
}

#[cfg(not(windows))]
fn foreground_input_language() -> Result<SpeechLanguage> {
    bail!("Follow-keyboard mode is unavailable on this platform; select English or Arabic in Settings")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_modes_resolve_without_a_foreground_window() {
        assert_eq!(
            resolve_speech_language(SpeechLanguageMode::English).unwrap(),
            SpeechLanguage::English
        );
        assert_eq!(
            resolve_speech_language(SpeechLanguageMode::Arabic).unwrap(),
            SpeechLanguage::Arabic
        );
    }
    #[test]
    fn keyboard_langids_select_only_english_and_arabic() {
        assert_eq!(
            language_from_langid(0x0401).unwrap(),
            SpeechLanguage::Arabic
        );
        assert_eq!(
            language_from_langid(0x0c01).unwrap(),
            SpeechLanguage::Arabic
        );
        assert_eq!(
            language_from_langid(0x0409).unwrap(),
            SpeechLanguage::English
        );
        assert!(language_from_langid(0x040c).is_err());
    }
}
