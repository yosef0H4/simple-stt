use anyhow::{anyhow, bail, Result};
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, GetAsyncKeyState, MapVirtualKeyW, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE,
    VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN,
    VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG,
    PM_NOREMOVE, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const MOD_CAPSLOCK: u8 = 0b00001;
const MOD_CTRL: u8 = 0b00010;
const MOD_SHIFT: u8 = 0b00100;
const MOD_WIN: u8 = 0b01000;
const MOD_LALT: u8 = 0b10000;
const MOD_RALT: u8 = 0b100000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    HotkeyDown,
    HotkeyUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyCaptureEvent {
    Preview(String),
    Complete(HotkeySpec),
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySpec {
    pub label: String,
    pub modifiers: u8,
    pub final_vk: u32,
}

impl Default for HotkeySpec {
    fn default() -> Self {
        Self::parse("capslock+s").expect("default hotkey must parse")
    }
}

impl HotkeySpec {
    pub fn parse(input: &str) -> Result<Self> {
        let tokens = input
            .trim()
            .split('+')
            .map(|token| token.trim().to_ascii_lowercase())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            bail!("hotkey string is empty");
        }

        let mut modifiers = 0_u8;
        let mut final_vk = None;
        let mut final_label = None;
        for token in &tokens {
            if let Some(bit) = modifier_bit(token) {
                modifiers |= bit;
                continue;
            }
            let vk = key_token_to_vk(token)
                .ok_or_else(|| anyhow!("unsupported hotkey key: {token:?}"))?;
            if final_vk.replace(vk).is_some() {
                bail!("hotkey must contain exactly one non-modifier key");
            }
            final_label = Some(key_label(vk).to_owned());
        }

        let Some(final_vk) = final_vk else {
            bail!("hotkey must end with one non-modifier key");
        };
        if modifiers == 0 {
            bail!("hotkey must include at least one modifier");
        }

        let mut parts = Vec::new();
        if modifiers & MOD_CTRL != 0 {
            parts.push("Ctrl".to_owned());
        }
        if modifiers & MOD_SHIFT != 0 {
            parts.push("Shift".to_owned());
        }
        if modifiers & MOD_WIN != 0 {
            parts.push("Win".to_owned());
        }
        if modifiers & MOD_LALT != 0 {
            parts.push("LAlt".to_owned());
        }
        if modifiers & MOD_RALT != 0 {
            parts.push("RAlt".to_owned());
        }
        if modifiers & MOD_CAPSLOCK != 0 {
            parts.push("CapsLock".to_owned());
        }
        parts.push(final_label.unwrap_or_else(|| key_label(final_vk).to_owned()));

        Ok(Self {
            label: parts.join("+"),
            modifiers,
            final_vk,
        })
    }

    pub fn uses_capslock(&self) -> bool {
        self.modifiers & MOD_CAPSLOCK != 0
    }

    fn modifier_vks(&self) -> Vec<u32> {
        let mut keys = Vec::new();
        if self.modifiers & MOD_CAPSLOCK != 0 {
            keys.push(VK_CAPITAL as u32);
        }
        if self.modifiers & MOD_CTRL != 0 {
            keys.extend([VK_LCONTROL as u32, VK_RCONTROL as u32]);
        }
        if self.modifiers & MOD_SHIFT != 0 {
            keys.extend([VK_LSHIFT as u32, VK_RSHIFT as u32]);
        }
        if self.modifiers & MOD_LALT != 0 {
            keys.push(VK_LMENU as u32);
        }
        if self.modifiers & MOD_RALT != 0 {
            keys.push(VK_RMENU as u32);
        }
        if self.modifiers & MOD_WIN != 0 {
            keys.extend([VK_LWIN as u32, VK_RWIN as u32]);
        }
        keys
    }
}

#[derive(Debug, Default)]
struct HookState {
    down_modifiers: u8,
    down_keys: HashSet<u32>,
    active: bool,
    pending_capslock: bool,
    capslock_consumed: bool,
    repaired_modifiers: HashSet<u32>,
}

impl HookState {
    fn update_key_state(&mut self, physical_vk: u32, down: bool) {
        if down {
            self.down_keys.insert(physical_vk);
        } else {
            self.down_keys.remove(&physical_vk);
        }

        if let Some(bit) = modifier_bit_for_vk(physical_vk) {
            if down {
                self.down_modifiers |= bit;
            } else if !same_neutral_modifier_still_down(bit, &self.down_keys) {
                self.down_modifiers &= !bit;
            }
        }
    }

    fn exact_match(&self, spec: &HotkeySpec) -> bool {
        exact_match_from_parts(spec, self.down_modifiers, self.down_keys.iter().copied())
    }

    fn should_suppress_while_active(&self, spec: &HotkeySpec, physical_vk: u32) -> bool {
        physical_vk == spec.final_vk || spec.modifier_vks().contains(&physical_vk)
    }

    fn reset_active(&mut self) {
        self.active = false;
        self.repaired_modifiers.clear();
    }
}

static EVENT_TX: OnceLock<Sender<HotkeyEvent>> = OnceLock::new();
static HOTKEY_ENABLED: AtomicBool = AtomicBool::new(true);
static SPEC: OnceLock<Mutex<HotkeySpec>> = OnceLock::new();
static STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let info = &*(lparam as *const KBDLLHOOKSTRUCT);
    if info.flags & LLKHF_INJECTED != 0 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    if !HOTKEY_ENABLED.load(Ordering::SeqCst) {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }

    let is_down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
    if !is_down && !is_up {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }

    let spec = SPEC
        .get()
        .and_then(|spec| spec.lock().ok().map(|spec| spec.clone()))
        .unwrap_or_default();
    let physical_vk = resolve_physical_vk(info);
    let physical_modifiers = current_modifier_bits();
    let mut suppress = false;
    let mut event = None;
    let mut replay_capslock = false;

    let state_lock = STATE.get_or_init(|| Mutex::new(HookState::default()));
    {
        let mut state = state_lock.lock().unwrap();
        let was_down = state.down_keys.contains(&physical_vk);
        state.update_key_state(physical_vk, is_down);

        if spec.uses_capslock() && physical_vk == VK_CAPITAL as u32 {
            suppress = true;
            if is_down && !was_down {
                state.pending_capslock = true;
                state.capslock_consumed = false;
            } else if is_up && state.pending_capslock {
                if state.capslock_consumed {
                    if crate::config::AppConfig::load()
                        .map(|config| config.capslock_always_off)
                        .unwrap_or(false)
                    {
                        crate::input::set_capslock_state(false);
                    }
                } else {
                    replay_capslock = true;
                }
                state.pending_capslock = false;
                state.capslock_consumed = false;
            }
        }

        if state.active {
            if state.should_suppress_while_active(&spec, physical_vk) {
                suppress = true;
                if is_up && modifier_bit_for_vk(physical_vk).is_some() {
                    state.repaired_modifiers.insert(physical_vk);
                }
            }
            if is_up && physical_vk == spec.final_vk {
                repair_suppressed_modifiers(&state.repaired_modifiers);
                force_release_hotkey_keys(&spec);
                force_release_all_modifiers();
                state.reset_active();
                event = Some(HotkeyEvent::HotkeyUp);
            }
        }
        let hook_match = state.exact_match(&spec);
        let physical_match =
            exact_match_from_parts(&spec, physical_modifiers, std::iter::once(physical_vk));
        let relevant = physical_vk == spec.final_vk || modifier_bit_for_vk(physical_vk).is_some();
        if relevant {
            tracing::debug!(
                vk = physical_vk,
                key = key_label(physical_vk),
                is_down,
                is_up,
                was_down,
                active = state.active,
                hook_modifiers = state.down_modifiers,
                physical_modifiers,
                expected_modifiers = spec.modifiers,
                hook_match,
                physical_match,
                hotkey = %spec.label,
                "runtime hotkey event"
            );
        }

        if !state.active
            && is_down
            && physical_vk == spec.final_vk
            && !was_down
            && (hook_match || physical_match)
        {
            suppress = true;
            state.active = true;
            state.capslock_consumed = spec.uses_capslock();
            state.repaired_modifiers.clear();
            event = Some(HotkeyEvent::HotkeyDown);
        }
    }

    if replay_capslock {
        replay_capslock_tap();
    }
    if let Some(event) = event {
        if let Some(tx) = EVENT_TX.get() {
            let _ = tx.send(event);
        }
    }
    if suppress {
        1
    } else {
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

pub fn set_enabled(enabled: bool) {
    HOTKEY_ENABLED.store(enabled, Ordering::SeqCst);
    let spec = SPEC
        .get()
        .and_then(|spec| spec.lock().ok().map(|spec| spec.clone()))
        .unwrap_or_default();
    force_release_hotkey_keys(&spec);
    force_release_all_modifiers();
    if !enabled {
        if let Some(state) = STATE.get() {
            *state.lock().unwrap() = HookState::default();
        }
    }
}

pub fn set_record_hotkey(spec: HotkeySpec) {
    let previous = SPEC
        .get()
        .and_then(|spec| spec.lock().ok().map(|spec| spec.clone()))
        .unwrap_or_default();
    force_release_hotkey_keys(&previous);
    force_release_all_modifiers();
    let lock = SPEC.get_or_init(|| Mutex::new(HotkeySpec::default()));
    *lock.lock().unwrap() = spec;
    if let Some(state) = STATE.get() {
        *state.lock().unwrap() = HookState::default();
    }
}

pub fn spawn_hotkey_hook(
    spec: HotkeySpec,
    tx: Sender<HotkeyEvent>,
) -> Result<thread::JoinHandle<()>> {
    EVENT_TX
        .set(tx)
        .map_err(|_| anyhow!("hotkey hook already initialized"))?;
    set_record_hotkey(spec.clone());
    force_release_all_modifiers();
    Ok(thread::spawn(move || unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), null_mut(), 0);
        if hook.is_null() {
            tracing::error!("SetWindowsHookExW failed; global hotkey capture is unavailable");
            return;
        }
        tracing::info!(hotkey = spec.label, "hold-to-record hotkey hook installed");
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    }))
}

#[derive(Debug)]
pub struct HotkeyCaptureHandle {
    thread_id: Arc<Mutex<Option<u32>>>,
    cancel: Arc<AtomicBool>,
}

impl HotkeyCaptureHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(thread_id) = *self.thread_id.lock().unwrap() {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_APP + 0x70, 0, 0);
            }
        }
    }
}

pub fn start_hotkey_capture() -> (HotkeyCaptureHandle, crossbeam_channel::Receiver<HotkeyCaptureEvent>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicBool::new(false));
    let thread_id = Arc::new(Mutex::new(None));
    let thread_cancel = Arc::clone(&cancel);
    let thread_completed = Arc::clone(&completed);
    let thread_id_out = Arc::clone(&thread_id);
    let thread_id_for_poll = Arc::clone(&thread_id);
    thread::spawn(move || unsafe {
        CAPTURE_DONE.store(false, Ordering::SeqCst);
        let thread_id = GetCurrentThreadId();
        *thread_id_out.lock().unwrap() = Some(thread_id);
        let capture = Box::into_raw(Box::new(CaptureState::new(tx, thread_cancel, thread_completed)));
        let mut message: MSG = zeroed();
        let _ = PeekMessageW(&mut message, null_mut(), WM_APP, WM_APP, PM_NOREMOVE);
        *CAPTURE_STATE.lock().unwrap() = capture as usize;
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(capture_proc), null_mut(), 0);
        if hook.is_null() {
            *CAPTURE_STATE.lock().unwrap() = 0;
            let capture = Box::from_raw(capture);
            let _ = capture
                .tx
                .send(HotkeyCaptureEvent::Error("SetWindowsHookExW failed".to_owned()));
            return;
        }
        tracing::debug!("hotkey settings capture hook installed");
        let poll_cancel = (*capture).cancel.clone();
        let poll_completed = (*capture).completed.clone();
        let poll_tx = (*capture).tx.clone();
        let poll_thread = thread::spawn(move || {
            poll_hotkey_capture(poll_tx, poll_cancel, poll_completed, thread_id_for_poll)
        });
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_APP + 0x70 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
            if CAPTURE_DONE.load(Ordering::SeqCst) {
                break;
            }
        }
        (*capture).cancel.store(true, Ordering::SeqCst);
        let _ = poll_thread.join();
        UnhookWindowsHookEx(hook);
        let ptr = {
            let mut value = CAPTURE_STATE.lock().unwrap();
            let ptr = *value as *mut CaptureState;
            *value = 0;
            ptr
        };
        if !ptr.is_null() {
            let capture = Box::from_raw(ptr);
            if capture.cancel.load(Ordering::SeqCst) && !capture.completed.load(Ordering::SeqCst) {
                let _ = capture.tx.send(HotkeyCaptureEvent::Cancelled);
            }
        }
        CAPTURE_DONE.store(false, Ordering::SeqCst);
        tracing::debug!("hotkey settings capture hook stopped");
    });
    (HotkeyCaptureHandle { thread_id, cancel }, rx)
}

struct CaptureState {
    tx: Sender<HotkeyCaptureEvent>,
    cancel: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}

impl CaptureState {
    fn new(tx: Sender<HotkeyCaptureEvent>, cancel: Arc<AtomicBool>, completed: Arc<AtomicBool>) -> Self {
        Self { tx, cancel, completed }
    }
}

static CAPTURE_STATE: Mutex<usize> = Mutex::new(0);
static CAPTURE_DONE: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn capture_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let ptr = *CAPTURE_STATE.lock().unwrap() as *mut CaptureState;
    if ptr.is_null() {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let state = &mut *ptr;
    let info = &*(lparam as *const KBDLLHOOKSTRUCT);
    if info.flags & LLKHF_INJECTED != 0 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    if state.cancel.load(Ordering::SeqCst) {
        CAPTURE_DONE.store(true, Ordering::SeqCst);
        return 1;
    }
    if state.completed.load(Ordering::SeqCst) || CAPTURE_DONE.load(Ordering::SeqCst) {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let is_down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
    if !is_down && !is_up {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }
    let vk = resolve_physical_vk(info);
    tracing::debug!(
        vk,
        physical_vk = vk,
        scan_code = info.scanCode,
        flags = info.flags,
        event = if is_down { "down" } else { "up" },
        label = key_label(vk),
        "hotkey settings capture raw keyboard event"
    );
    1
}

fn poll_hotkey_capture(
    tx: Sender<HotkeyCaptureEvent>,
    cancel: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    thread_id: Arc<Mutex<Option<u32>>>,
) {
    let mut preview = String::new();
    while !cancel.load(Ordering::SeqCst) && !CAPTURE_DONE.load(Ordering::SeqCst) {
        let modifiers = current_modifier_bits();
        let final_vk = capture_final_keys()
            .into_iter()
            .find(|vk| is_key_physically_down(*vk));
        let label = match (modifiers, final_vk) {
            (0, Some(vk)) => key_label(vk).to_owned(),
            (0, None) => String::new(),
            (mods, Some(vk)) => label_from_parts(mods, vk),
            (mods, None) => modifier_label(mods),
        };

        if label != preview {
            preview = label.clone();
            tracing::debug!(
                preview,
                modifiers,
                final_vk = final_vk.unwrap_or_default(),
                "hotkey settings capture poll preview"
            );
            let _ = tx.send(HotkeyCaptureEvent::Preview(label.clone()));
        }

        if modifiers != 0 {
            if let Some(vk) = final_vk {
                let label = label_from_parts(modifiers, vk);
                match HotkeySpec::parse(&label) {
                    Ok(spec) => {
                        tracing::info!(hotkey = spec.label, "hotkey settings capture completed");
                        completed.store(true, Ordering::SeqCst);
                        let _ = tx.send(HotkeyCaptureEvent::Preview(spec.label.clone()));
                        let _ = tx.send(HotkeyCaptureEvent::Complete(spec));
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, label, "hotkey settings capture rejected chord");
                        let _ = tx.send(HotkeyCaptureEvent::Error(error.to_string()));
                    }
                }
                CAPTURE_DONE.store(true, Ordering::SeqCst);
                if let Some(thread_id) = *thread_id.lock().unwrap() {
                    unsafe {
                        let _ = PostThreadMessageW(thread_id, WM_APP + 0x70, 0, 0);
                    }
                }
                break;
            }
        }

        thread::sleep(Duration::from_millis(12));
    }
}

fn current_modifier_bits() -> u8 {
    let mut modifiers = 0;
    if is_key_physically_down(VK_CAPITAL as u32) {
        modifiers |= MOD_CAPSLOCK;
    }
    if is_key_physically_down(VK_CONTROL as u32)
        || is_key_physically_down(VK_LCONTROL as u32)
        || is_key_physically_down(VK_RCONTROL as u32)
    {
        modifiers |= MOD_CTRL;
    }
    if is_key_physically_down(VK_SHIFT as u32)
        || is_key_physically_down(VK_LSHIFT as u32)
        || is_key_physically_down(VK_RSHIFT as u32)
    {
        modifiers |= MOD_SHIFT;
    }
    if is_key_physically_down(VK_LMENU as u32) {
        modifiers |= MOD_LALT;
    }
    if is_key_physically_down(VK_RMENU as u32) {
        modifiers |= MOD_RALT;
    }
    if is_key_physically_down(VK_LWIN as u32) || is_key_physically_down(VK_RWIN as u32) {
        modifiers |= MOD_WIN;
    }
    modifiers
}

fn exact_match_from_parts(
    spec: &HotkeySpec,
    modifiers: u8,
    keys: impl IntoIterator<Item = u32>,
) -> bool {
    let modifiers = normalized_match_modifiers(spec, modifiers);
    if modifiers != spec.modifiers {
        return false;
    }
    let mut saw_final = false;
    for vk in keys {
        if vk == spec.final_vk {
            saw_final = true;
        } else if modifier_bit_for_vk(vk).is_none() || !modifier_vk_allowed_for_spec(spec, vk) {
            return false;
        }
    }
    saw_final
}

fn normalized_match_modifiers(spec: &HotkeySpec, modifiers: u8) -> u8 {
    if spec.modifiers & MOD_RALT != 0 && spec.modifiers & MOD_CTRL == 0 {
        modifiers & !MOD_CTRL
    } else {
        modifiers
    }
}

fn modifier_vk_allowed_for_spec(spec: &HotkeySpec, vk: u32) -> bool {
    let Some(bit) = modifier_bit_for_vk(vk) else {
        return false;
    };
    if spec.modifiers & bit != 0 {
        return true;
    }
    bit == MOD_CTRL && spec.modifiers & MOD_RALT != 0 && spec.modifiers & MOD_CTRL == 0
}

fn is_key_physically_down(vk: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn capture_final_keys() -> Vec<u32> {
    let mut keys = Vec::new();
    keys.extend(0x41..=0x5A);
    keys.extend(0x30..=0x39);
    keys.extend([
        VK_ESCAPE as u32,
        VK_RETURN as u32,
        VK_BACK as u32,
        VK_SPACE as u32,
        VK_TAB as u32,
        VK_INSERT as u32,
        VK_DELETE as u32,
        VK_HOME as u32,
        VK_END as u32,
        VK_PRIOR as u32,
        VK_NEXT as u32,
        VK_UP as u32,
        VK_DOWN as u32,
        VK_LEFT as u32,
        VK_RIGHT as u32,
    ]);
    keys.extend((VK_F1 as u32)..=(VK_F12 as u32));
    keys
}

fn modifier_bit(token: &str) -> Option<u8> {
    match token {
        "capslock" | "caps" | "caps_lock" => Some(MOD_CAPSLOCK),
        "ctrl" | "control" => Some(MOD_CTRL),
        "shift" => Some(MOD_SHIFT),
        "alt" | "lalt" | "leftalt" | "left_alt" => Some(MOD_LALT),
        "ralt" | "rightalt" | "right_alt" | "altgr" => Some(MOD_RALT),
        "win" | "windows" | "meta" => Some(MOD_WIN),
        _ => None,
    }
}

fn key_token_to_vk(token: &str) -> Option<u32> {
    if token.len() == 1 {
        let byte = token.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some(byte.to_ascii_uppercase() as u32);
        }
        if byte.is_ascii_digit() {
            return Some(byte as u32);
        }
    }
    match token {
        "esc" | "escape" => Some(VK_ESCAPE as u32),
        "enter" | "return" => Some(VK_RETURN as u32),
        "backspace" => Some(VK_BACK as u32),
        "space" => Some(VK_SPACE as u32),
        "tab" => Some(VK_TAB as u32),
        "insert" => Some(VK_INSERT as u32),
        "delete" => Some(VK_DELETE as u32),
        "home" => Some(VK_HOME as u32),
        "end" => Some(VK_END as u32),
        "pageup" | "pgup" => Some(VK_PRIOR as u32),
        "pagedown" | "pgdn" => Some(VK_NEXT as u32),
        "up" => Some(VK_UP as u32),
        "down" => Some(VK_DOWN as u32),
        "left" => Some(VK_LEFT as u32),
        "right" => Some(VK_RIGHT as u32),
        _ => {
            let number = token.strip_prefix('f')?.parse::<u32>().ok()?;
            (1..=12)
                .contains(&number)
                .then_some(VK_F1 as u32 + number - 1)
        }
    }
}

fn key_label(vk: u32) -> &'static str {
    match vk {
        0x41 => "A",
        0x42 => "B",
        0x43 => "C",
        0x44 => "D",
        0x45 => "E",
        0x46 => "F",
        0x47 => "G",
        0x48 => "H",
        0x49 => "I",
        0x4A => "J",
        0x4B => "K",
        0x4C => "L",
        0x4D => "M",
        0x4E => "N",
        0x4F => "O",
        0x50 => "P",
        0x51 => "Q",
        0x52 => "R",
        0x53 => "S",
        0x54 => "T",
        0x55 => "U",
        0x56 => "V",
        0x57 => "W",
        0x58 => "X",
        0x59 => "Y",
        0x5A => "Z",
        0x30 => "0",
        0x31 => "1",
        0x32 => "2",
        0x33 => "3",
        0x34 => "4",
        0x35 => "5",
        0x36 => "6",
        0x37 => "7",
        0x38 => "8",
        0x39 => "9",
        x if x == VK_ESCAPE as u32 => "Esc",
        x if x == VK_RETURN as u32 => "Enter",
        x if x == VK_BACK as u32 => "Backspace",
        x if x == VK_SPACE as u32 => "Space",
        x if x == VK_TAB as u32 => "Tab",
        x if x == VK_INSERT as u32 => "Insert",
        x if x == VK_DELETE as u32 => "Delete",
        x if x == VK_HOME as u32 => "Home",
        x if x == VK_END as u32 => "End",
        x if x == VK_PRIOR as u32 => "PageUp",
        x if x == VK_NEXT as u32 => "PageDown",
        x if x == VK_UP as u32 => "Up",
        x if x == VK_DOWN as u32 => "Down",
        x if x == VK_LEFT as u32 => "Left",
        x if x == VK_RIGHT as u32 => "Right",
        x if x == VK_CAPITAL as u32 => "CapsLock",
        x if x == VK_CONTROL as u32 => "Ctrl",
        x if x == VK_LCONTROL as u32 => "LCtrl",
        x if x == VK_RCONTROL as u32 => "RCtrl",
        x if x == VK_SHIFT as u32 => "Shift",
        x if x == VK_LSHIFT as u32 => "LShift",
        x if x == VK_RSHIFT as u32 => "RShift",
        x if x == VK_MENU as u32 => "LAlt",
        x if x == VK_LMENU as u32 => "LAlt",
        x if x == VK_RMENU as u32 => "RAlt",
        x if x == VK_LWIN as u32 => "LWin",
        x if x == VK_RWIN as u32 => "RWin",
        x if x == VK_F1 as u32 => "F1",
        x if x == VK_F1 as u32 + 1 => "F2",
        x if x == VK_F1 as u32 + 2 => "F3",
        x if x == VK_F1 as u32 + 3 => "F4",
        x if x == VK_F1 as u32 + 4 => "F5",
        x if x == VK_F1 as u32 + 5 => "F6",
        x if x == VK_F1 as u32 + 6 => "F7",
        x if x == VK_F1 as u32 + 7 => "F8",
        x if x == VK_F1 as u32 + 8 => "F9",
        x if x == VK_F1 as u32 + 9 => "F10",
        x if x == VK_F1 as u32 + 10 => "F11",
        x if x == VK_F12 as u32 => "F12",
        _ => "Unknown",
    }
}

fn label_from_parts(modifiers: u8, final_vk: u32) -> String {
    let mut parts = Vec::new();
    if modifiers & MOD_CTRL != 0 {
        parts.push("Ctrl");
    }
    if modifiers & MOD_SHIFT != 0 {
        parts.push("Shift");
    }
    if modifiers & MOD_WIN != 0 {
        parts.push("Win");
    }
    if modifiers & MOD_LALT != 0 {
        parts.push("LAlt");
    }
    if modifiers & MOD_RALT != 0 {
        parts.push("RAlt");
    }
    if modifiers & MOD_CAPSLOCK != 0 {
        parts.push("CapsLock");
    }
    parts.push(key_label(final_vk));
    parts.join("+")
}

fn modifier_label(modifiers: u8) -> String {
    let mut parts = Vec::new();
    if modifiers & MOD_CTRL != 0 {
        parts.push("Ctrl");
    }
    if modifiers & MOD_SHIFT != 0 {
        parts.push("Shift");
    }
    if modifiers & MOD_WIN != 0 {
        parts.push("Win");
    }
    if modifiers & MOD_LALT != 0 {
        parts.push("LAlt");
    }
    if modifiers & MOD_RALT != 0 {
        parts.push("RAlt");
    }
    if modifiers & MOD_CAPSLOCK != 0 {
        parts.push("CapsLock");
    }
    parts.join("+")
}

fn modifier_bit_for_vk(vk: u32) -> Option<u8> {
    match vk {
        x if x == VK_CAPITAL as u32 => Some(MOD_CAPSLOCK),
        x if x == VK_CONTROL as u32 || x == VK_LCONTROL as u32 || x == VK_RCONTROL as u32 => {
            Some(MOD_CTRL)
        }
        x if x == VK_SHIFT as u32 || x == VK_LSHIFT as u32 || x == VK_RSHIFT as u32 => {
            Some(MOD_SHIFT)
        }
        x if x == VK_MENU as u32 || x == VK_LMENU as u32 => Some(MOD_LALT),
        x if x == VK_RMENU as u32 => Some(MOD_RALT),
        x if x == VK_LWIN as u32 || x == VK_RWIN as u32 => Some(MOD_WIN),
        _ => None,
    }
}

fn same_neutral_modifier_still_down(bit: u8, down_keys: &HashSet<u32>) -> bool {
    down_keys
        .iter()
        .copied()
        .any(|vk| modifier_bit_for_vk(vk) == Some(bit))
}

fn resolve_physical_vk(info: &KBDLLHOOKSTRUCT) -> u32 {
    let vk = info.vkCode;
    if vk == VK_CONTROL as u32 {
        return if info.flags & LLKHF_EXTENDED != 0 {
            VK_RCONTROL as u32
        } else {
            VK_LCONTROL as u32
        };
    }
    if vk == VK_MENU as u32 {
        return if info.flags & LLKHF_EXTENDED != 0 {
            VK_RMENU as u32
        } else {
            VK_LMENU as u32
        };
    }
    if vk == VK_SHIFT as u32 {
        let mapped = unsafe { MapVirtualKeyW(info.scanCode, 3) };
        if mapped == VK_LSHIFT as u32 || mapped == VK_RSHIFT as u32 {
            return mapped;
        }
    }
    vk
}

fn repair_suppressed_modifiers(modifiers: &HashSet<u32>) {
    for vk in modifiers.iter().copied() {
        if modifier_bit_for_vk(vk).is_some() {
            send_key_up(vk);
        }
    }
}

fn force_release_hotkey_keys(spec: &HotkeySpec) {
    for vk in spec.modifier_vks().into_iter().chain([spec.final_vk]) {
        send_key_up(vk);
    }
    tracing::debug!(hotkey = %spec.label, "sent hotkey key-up repair events");
}

fn force_release_all_modifiers() {
    for vk in [
        VK_CAPITAL as u32,
        VK_LCONTROL as u32,
        VK_RCONTROL as u32,
        VK_LSHIFT as u32,
        VK_RSHIFT as u32,
        VK_LMENU as u32,
        VK_RMENU as u32,
        VK_LWIN as u32,
        VK_RWIN as u32,
    ] {
        send_key_up(vk);
    }
    tracing::debug!("sent global modifier key-up repair events");
}

fn send_key_up(vk: u32) {
    let extended = matches!(
        vk,
        x if x == VK_RMENU as u32
            || x == VK_RCONTROL as u32
            || x == VK_LWIN as u32
            || x == VK_RWIN as u32
            || x == VK_INSERT as u32
            || x == VK_DELETE as u32
            || x == VK_HOME as u32
            || x == VK_END as u32
            || x == VK_PRIOR as u32
            || x == VK_NEXT as u32
            || x == VK_UP as u32
            || x == VK_DOWN as u32
            || x == VK_LEFT as u32
            || x == VK_RIGHT as u32
    );
    let flags = 0x0002 | if extended { 0x0001 } else { 0 };
    unsafe {
        keybd_event(vk as u8, 0, flags, 0);
    }
}

fn replay_capslock_tap() {
    unsafe {
        keybd_event(VK_CAPITAL as u8, 0x45, 0, 0);
        keybd_event(VK_CAPITAL as u8, 0x45, 0x0002, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_hotkeys() {
        assert_eq!(HotkeySpec::parse("capslock+s").unwrap().label, "CapsLock+S");
        assert_eq!(
            HotkeySpec::parse("Ctrl+Win+CapsLock+A").unwrap().label,
            "Ctrl+Win+CapsLock+A"
        );
        assert_eq!(HotkeySpec::parse("shift+f12").unwrap().label, "Shift+F12");
        assert_eq!(HotkeySpec::parse("lalt+a").unwrap().label, "LAlt+A");
        assert_eq!(HotkeySpec::parse("ralt+a").unwrap().label, "RAlt+A");
        assert_eq!(HotkeySpec::parse("alt+a").unwrap().label, "LAlt+A");
    }

    #[test]
    fn rejects_invalid_hotkeys() {
        for value in ["", "capslock", "ctrl+shift", "s", "capslock+ctrl", "ctrl+a+b"] {
            assert!(HotkeySpec::parse(value).is_err(), "{value} should fail");
        }
    }

    #[test]
    fn exact_match_requires_no_extra_modifiers() {
        let spec = HotkeySpec::parse("capslock+s").unwrap();
        let mut state = HookState::default();
        state.update_key_state(VK_CAPITAL as u32, true);
        state.update_key_state('S' as u32, true);
        assert!(state.exact_match(&spec));
        state.update_key_state(VK_LSHIFT as u32, true);
        assert!(!state.exact_match(&spec));
    }

    #[test]
    fn left_and_right_alt_are_distinct_modifiers() {
        let left = HotkeySpec::parse("lalt+a").unwrap();
        let right = HotkeySpec::parse("ralt+a").unwrap();
        assert_ne!(left.modifiers, right.modifiers);

        let mut state = HookState::default();
        state.update_key_state(VK_LMENU as u32, true);
        state.update_key_state('A' as u32, true);
        assert!(state.exact_match(&left));
        assert!(!state.exact_match(&right));

        let mut state = HookState::default();
        state.update_key_state(VK_RMENU as u32, true);
        state.update_key_state('A' as u32, true);
        assert!(state.exact_match(&right));
        assert!(!state.exact_match(&left));
    }

    #[test]
    fn right_alt_allows_windows_altgr_synthetic_ctrl() {
        let spec = HotkeySpec::parse("ralt+z").unwrap();
        assert!(exact_match_from_parts(
            &spec,
            MOD_CTRL | MOD_RALT,
            [VK_RMENU as u32, VK_LCONTROL as u32, 'Z' as u32]
        ));

        let left = HotkeySpec::parse("lalt+z").unwrap();
        assert!(!exact_match_from_parts(
            &left,
            MOD_CTRL | MOD_LALT,
            [VK_LMENU as u32, VK_LCONTROL as u32, 'Z' as u32]
        ));
    }

    #[test]
    fn configured_modifiers_pass_through_before_final_key() {
        let mut state = HookState::default();
        state.update_key_state(VK_LWIN as u32, true);
        assert!(!state.active);
        assert!(!state.exact_match(&HotkeySpec::parse("win+z").unwrap()));
    }

    #[test]
    fn physical_exact_match_accepts_win_modifier_and_rejects_extras() {
        let spec = HotkeySpec::parse("ctrl+shift+win+z").unwrap();
        assert!(exact_match_from_parts(
            &spec,
            MOD_CTRL | MOD_SHIFT | MOD_WIN,
            ['Z' as u32]
        ));
        assert!(!exact_match_from_parts(
            &spec,
            MOD_CTRL | MOD_SHIFT | MOD_WIN | MOD_LALT,
            ['Z' as u32]
        ));
        assert!(!exact_match_from_parts(
            &spec,
            MOD_CTRL | MOD_SHIFT | MOD_WIN,
            ['Z' as u32, 'X' as u32]
        ));
    }

    #[test]
    fn final_key_release_is_detectable_once() {
        let spec = HotkeySpec::parse("capslock+s").unwrap();
        let mut state = HookState::default();
        state.update_key_state(VK_CAPITAL as u32, true);
        state.update_key_state('S' as u32, true);
        assert!(state.exact_match(&spec));
        state.active = true;
        state.update_key_state('S' as u32, false);
        assert!(state.should_suppress_while_active(&spec, 'S' as u32));
        state.reset_active();
        assert!(!state.active);
    }

    #[test]
    fn capslock_alone_can_pass_through_if_not_consumed() {
        let mut state = HookState::default();
        state.pending_capslock = true;
        state.capslock_consumed = false;
        assert!(state.pending_capslock && !state.capslock_consumed);
        state.capslock_consumed = true;
        assert!(state.pending_capslock && state.capslock_consumed);
    }

    #[test]
    fn capslock_consumed_survives_final_key_release() {
        let mut state = HookState::default();
        state.pending_capslock = true;
        state.capslock_consumed = true;
        state.active = true;
        state.reset_active();
        assert!(state.capslock_consumed);
    }

    #[test]
    fn live_capture_labels_modifiers_and_full_chord() {
        assert_eq!(modifier_label(MOD_CAPSLOCK), "CapsLock");
        assert_eq!(modifier_label(MOD_CTRL | MOD_WIN | MOD_CAPSLOCK), "Ctrl+Win+CapsLock");
        assert_eq!(label_from_parts(MOD_CTRL | MOD_WIN | MOD_CAPSLOCK, 'A' as u32), "Ctrl+Win+CapsLock+A");
        assert_eq!(label_from_parts(MOD_RALT, 'A' as u32), "RAlt+A");
    }

    #[test]
    fn capture_final_key_set_contains_common_keyboard_keys() {
        let keys = capture_final_keys();
        for vk in ['A' as u32, 'S' as u32, '0' as u32, VK_F12 as u32, VK_RETURN as u32] {
            assert!(keys.contains(&vk), "capture key set should contain {vk}");
        }
        for vk in [VK_CAPITAL as u32, VK_LSHIFT as u32, VK_LCONTROL as u32, VK_LMENU as u32, VK_RMENU as u32, VK_LWIN as u32] {
            assert!(!keys.contains(&vk), "modifiers should not be final keys");
        }
    }
}
