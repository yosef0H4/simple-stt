//! Bounded physical keyboard actions; no desktop hooks or input libraries.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub const MAX_EVENTS: usize = 128;
pub const MAX_DURATION_MS: u32 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub key: String,
    pub down: bool,
    pub at_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedAction {
    pub events: Vec<KeyEvent>,
}

impl RecordedAction {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.events.is_empty() && self.events.len() <= MAX_EVENTS,
            "recorded action must contain 1 to 128 events"
        );
        ensure!(
            self.events[0].at_ms == 0,
            "recorded action must start at its first input"
        );
        let mut held: Vec<&str> = Vec::new();
        let mut previous = 0;
        for event in &self.events {
            keycode(&event.key)?;
            ensure!(
                event.at_ms >= previous && event.at_ms <= MAX_DURATION_MS,
                "recorded action timing must be ordered and at most 30 seconds"
            );
            previous = event.at_ms;
            if event.down {
                ensure!(
                    !held.contains(&event.key.as_str()),
                    "recorded key was already pressed"
                );
                held.push(event.key.as_str());
            } else {
                let index = held
                    .iter()
                    .position(|key| *key == event.key.as_str())
                    .ok_or_else(|| anyhow::anyhow!("recorded key release has no press"))?;
                held.remove(index);
            }
        }
        ensure!(held.is_empty(), "recorded action must release every key");
        Ok(())
    }
}

/// Offsets are absolute, so input delivery overhead does not accumulate as gaps.
pub trait Playback {
    fn wait_until(&mut self, at_ms: u32) -> Result<()>;
    fn key(&mut self, key: &str, down: bool) -> Result<()>;
}

pub fn replay(action: &RecordedAction, output: &mut impl Playback) -> Result<()> {
    action.validate()?;
    let mut held: Vec<&str> = Vec::new();
    let result = (|| {
        for event in &action.events {
            output.wait_until(event.at_ms)?;
            // A failed send can have delivered its press. Cleanup must try its release.
            if event.down && !held.contains(&event.key.as_str()) {
                held.push(event.key.as_str());
            }
            output.key(&event.key, event.down)?;
            if !event.down {
                held.retain(|key| *key != event.key);
            }
        }
        Ok(())
    })();
    let mut cleanup = Ok(());
    for key in held.into_iter().rev() {
        if let Err(error) = output.key(key, false) {
            cleanup = Err(error);
        }
    }
    result.and(cleanup)
}

/// DOM physical codes map to Linux evdev codes independently of keyboard layout.
pub fn keycode(key: &str) -> Result<u32> {
    let code = match key {
        "Escape" => 1,
        "Digit1" => 2,
        "Digit2" => 3,
        "Digit3" => 4,
        "Digit4" => 5,
        "Digit5" => 6,
        "Digit6" => 7,
        "Digit7" => 8,
        "Digit8" => 9,
        "Digit9" => 10,
        "Digit0" => 11,
        "Minus" => 12,
        "Equal" => 13,
        "Backspace" => 14,
        "Tab" => 15,
        "KeyQ" => 16,
        "KeyW" => 17,
        "KeyE" => 18,
        "KeyR" => 19,
        "KeyT" => 20,
        "KeyY" => 21,
        "KeyU" => 22,
        "KeyI" => 23,
        "KeyO" => 24,
        "KeyP" => 25,
        "BracketLeft" => 26,
        "BracketRight" => 27,
        "Enter" => 28,
        "ControlLeft" => 29,
        "KeyA" => 30,
        "KeyS" => 31,
        "KeyD" => 32,
        "KeyF" => 33,
        "KeyG" => 34,
        "KeyH" => 35,
        "KeyJ" => 36,
        "KeyK" => 37,
        "KeyL" => 38,
        "Semicolon" => 39,
        "Quote" => 40,
        "Backquote" => 41,
        "ShiftLeft" => 42,
        "Backslash" => 43,
        "KeyZ" => 44,
        "KeyX" => 45,
        "KeyC" => 46,
        "KeyV" => 47,
        "KeyB" => 48,
        "KeyN" => 49,
        "KeyM" => 50,
        "Comma" => 51,
        "Period" => 52,
        "Slash" => 53,
        "ShiftRight" => 54,
        "NumpadMultiply" => 55,
        "AltLeft" => 56,
        "Space" => 57,
        "CapsLock" => 58,
        "F1" => 59,
        "F2" => 60,
        "F3" => 61,
        "F4" => 62,
        "F5" => 63,
        "F6" => 64,
        "F7" => 65,
        "F8" => 66,
        "F9" => 67,
        "F10" => 68,
        "NumLock" => 69,
        "ScrollLock" => 70,
        "Numpad7" => 71,
        "Numpad8" => 72,
        "Numpad9" => 73,
        "NumpadSubtract" => 74,
        "Numpad4" => 75,
        "Numpad5" => 76,
        "Numpad6" => 77,
        "NumpadAdd" => 78,
        "Numpad1" => 79,
        "Numpad2" => 80,
        "Numpad3" => 81,
        "Numpad0" => 82,
        "NumpadDecimal" => 83,
        "IntlBackslash" => 86,
        "F11" => 87,
        "F12" => 88,
        "NumpadEnter" => 96,
        "ControlRight" => 97,
        "NumpadDivide" => 98,
        "PrintScreen" => 99,
        "AltRight" => 100,
        "Home" => 102,
        "ArrowUp" => 103,
        "PageUp" => 104,
        "ArrowLeft" => 105,
        "ArrowRight" => 106,
        "End" => 107,
        "ArrowDown" => 108,
        "PageDown" => 109,
        "Insert" => 110,
        "Delete" => 111,
        "MetaLeft" => 125,
        "MetaRight" => 126,
        "ContextMenu" => 127,
        _ => anyhow::bail!("unsupported recorded key"),
    };
    Ok(code)
}

pub fn windows_scan_code(key: &str) -> Result<u32> {
    Ok(match keycode(key)? {
        69 => 0x145,
        96 => 0x11c,
        97 => 0x11d,
        98 => 0x135,
        99 => 0x137,
        100 => 0x138,
        102 => 0x147,
        103 => 0x148,
        104 => 0x149,
        105 => 0x14b,
        106 => 0x14d,
        107 => 0x14f,
        108 => 0x150,
        109 => 0x151,
        110 => 0x152,
        111 => 0x153,
        125 => 0x15b,
        126 => 0x15c,
        127 => 0x15d,
        code => code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sequence() -> RecordedAction {
        RecordedAction {
            events: [
                ("ControlLeft", true, 0),
                ("Space", true, 40),
                ("Space", false, 70),
                ("Space", true, 260),
                ("Space", false, 300),
                ("ControlLeft", false, 310),
            ]
            .map(|(key, down, at_ms)| KeyEvent {
                key: key.into(),
                down,
                at_ms,
            })
            .to_vec(),
        }
    }
    #[derive(Default)]
    struct Fake {
        time: u32,
        cancel_at: Option<u32>,
        sent: Vec<(String, bool, u32)>,
    }
    impl Playback for Fake {
        fn wait_until(&mut self, at_ms: u32) -> Result<()> {
            ensure!(
                self.cancel_at.is_none_or(|at| at_ms < at),
                "canceled or focus changed"
            );
            self.time = at_ms;
            Ok(())
        }
        fn key(&mut self, key: &str, down: bool) -> Result<()> {
            self.sent.push((key.into(), down, self.time));
            Ok(())
        }
    }
    #[test]
    fn replay_preserves_overlaps_repeated_presses_and_internal_timing() {
        let action = sequence();
        let mut fake = Fake::default();
        replay(&action, &mut fake).unwrap();
        assert_eq!(
            fake.sent,
            action
                .events
                .iter()
                .map(|e| (e.key.clone(), e.down, e.at_ms))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn cancellation_releases_held_keys_without_waiting_for_the_rest() {
        let mut fake = Fake {
            cancel_at: Some(70),
            ..Default::default()
        };
        assert!(replay(&sequence(), &mut fake).is_err());
        assert_eq!(
            &fake.sent[2..],
            [
                ("Space".into(), false, 40),
                ("ControlLeft".into(), false, 40)
            ]
        );
    }
    #[test]
    fn malformed_actions_are_rejected_before_input() {
        let mut action = sequence();
        action.events.pop();
        assert!(action.validate().is_err());
        let mut action = sequence();
        action.events[0].at_ms = 1;
        assert!(action.validate().is_err());
        let mut action = sequence();
        action.events[2].at_ms = 39;
        assert!(action.validate().is_err());
        let mut action = sequence();
        action.events[1].key = "Unknown".into();
        assert!(action.validate().is_err());
        let mut action = sequence();
        action.events[5].at_ms = 30_001;
        assert!(action.validate().is_err());
        assert_eq!(windows_scan_code("ControlRight").unwrap(), 0x11d);
    }
}
