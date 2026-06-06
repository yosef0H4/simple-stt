use anyhow::{Context, Result};
use std::mem::zeroed;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadCursorW, MessageBoxW,
    PostQuitMessage, RegisterClassW, SetWindowTextW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MB_ICONINFORMATION, MB_OK, MSG, SW_SHOW, WM_COMMAND,
    WM_DESTROY, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

use crate::config::AppConfig;
use crate::models;
use crate::startup;
use crate::winutil::{loword, rgb, wide_null};

const CLASS: &str = "UvoxSettingsWindow";
const ID_SAVE: usize = 2001;
const ID_RESET: usize = 2002;
const ID_TEST: usize = 2003;
const ID_LOG: usize = 2004;
const ID_STARTUP: usize = 2005;
const ID_DOWNLOAD: usize = 2006;
const ID_MODEL_COMBO: usize = 2007;
const ID_RECORD_TEST: usize = 2008;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_GETLBTEXT: u32 = 0x0148;
const CB_SETCURSEL: u32 = 0x014E;

static mut MODEL_COMBO: HWND = null_mut();
static mut TEST_OUTPUT: HWND = null_mut();

pub fn show_settings() -> Result<()> {
    AppConfig::load()?.save()?;
    unsafe {
        let hwnd = create_window()?;
        ShowWindow(hwnd, SW_SHOW);
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

pub fn open_latest_log() -> Result<()> {
    std::process::Command::new("notepad.exe")
        .arg(AppConfig::log_path())
        .spawn()
        .context("opening latest log")?;
    Ok(())
}

unsafe fn create_window() -> Result<HWND> {
    let class = wide_null(CLASS);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: null_mut(),
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: CreateSolidBrush(rgb(245, 247, 250)),
        lpszMenuName: null_mut(),
        lpszClassName: class.as_ptr(),
        cbClsExtra: 0,
        cbWndExtra: 0,
    };
    RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        wide_null("Uvox Settings").as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        980,
        680,
        null_mut(),
        null_mut(),
        null_mut(),
        null_mut(),
    );
    anyhow::ensure!(!hwnd.is_null(), "creating settings window failed");
    build_controls(hwnd)?;
    Ok(hwnd)
}

unsafe fn build_controls(hwnd: HWND) -> Result<()> {
    let config = AppConfig::load()?;
    label(hwnd, "Uvox", 28, 24, 180, 28);
    for (idx, name) in ["General", "Audio", "Model", "Typing", "Logging", "Advanced"]
        .iter()
        .enumerate()
    {
        label(hwnd, name, 28, 82 + idx as i32 * 42, 160, 24);
    }

    label(hwnd, "Status", 240, 28, 160, 24);
    label(
        hwnd,
        "Ready. Hold CapsLock to record, release to transcribe.",
        240,
        58,
        620,
        24,
    );

    label(hwnd, "Audio", 240, 110, 160, 24);
    label(hwnd, "Microphone", 240, 142, 120, 22);
    edit(
        hwnd,
        &if config.audio_device_contains.is_empty() {
            "Default microphone".to_owned()
        } else {
            config.audio_device_contains.clone()
        },
        380,
        138,
        390,
        28,
    );
    label(hwnd, "Input level", 240, 184, 120, 22);
    label(
        hwnd,
        "[ live meter appears here while testing ]",
        380,
        184,
        320,
        22,
    );
    button(hwnd, "Record Test", ID_RECORD_TEST, 790, 138, 130, 32);

    label(hwnd, "Model", 240, 240, 160, 24);
    label(hwnd, "Active model", 240, 272, 120, 22);
    edit(hwnd, &config.parakeet_model_path, 380, 268, 390, 28);
    label(
        hwnd,
        "Catalog: all mudler/parakeet-cpp-gguf families; F16 recommended.",
        380,
        306,
        420,
        22,
    );
    button(hwnd, "Test Model", ID_TEST, 790, 268, 130, 32);
    combo(hwnd, 380, 334, 390, 180);
    button(hwnd, "Download Selected", ID_DOWNLOAD, 790, 334, 130, 32);

    label(hwnd, "Typing", 240, 366, 160, 24);
    label(
        hwnd,
        &format!("Chunk chars: {}", config.typing_chunk_chars),
        240,
        398,
        180,
        22,
    );
    label(
        hwnd,
        &format!("Interval: {} ms", config.typing_interval_ms),
        440,
        398,
        180,
        22,
    );
    label(
        hwnd,
        &format!("Unload timeout: {} sec", config.idle_timeout_secs),
        640,
        398,
        220,
        22,
    );

    label(hwnd, "Logging", 240, 462, 160, 24);
    label(
        hwnd,
        &format!("Current level: {:?}", config.log_level),
        240,
        494,
        250,
        22,
    );
    button(hwnd, "Open Latest Log", ID_LOG, 500, 490, 150, 32);
    label(hwnd, "Test transcript", 240, 526, 140, 22);
    output_edit(hwnd, "", 380, 522, 540, 46);

    button(hwnd, "Save", ID_SAVE, 240, 580, 110, 34);
    button(hwnd, "Reset", ID_RESET, 366, 580, 110, 34);
    button(
        hwnd,
        if config.start_with_windows {
            "Disable Startup"
        } else {
            "Start with Windows"
        },
        ID_STARTUP,
        492,
        580,
        170,
        34,
    );
    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            match loword(wparam as usize) as usize {
                ID_SAVE => {
                    if let Ok(config) = AppConfig::load() {
                        let _ = config.save();
                    }
                    info(hwnd, "Settings saved. Live Uvox will reload safe values.");
                }
                ID_RESET => {
                    let _ = AppConfig::default().save();
                    info(hwnd, "Settings reset. Reopen settings to refresh values.");
                }
                ID_TEST => {
                    test_model(hwnd);
                }
                ID_RECORD_TEST => {
                    record_test(hwnd);
                }
                ID_LOG => {
                    let _ = open_latest_log();
                }
                ID_STARTUP => {
                    if let Ok(mut config) = AppConfig::load() {
                        config.start_with_windows = !config.start_with_windows;
                        let _ = startup::set_start_with_windows(config.start_with_windows);
                        let _ = config.save();
                        info(hwnd, "Startup setting changed.");
                    }
                }
                ID_DOWNLOAD => {
                    download_recommended(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn record_test(hwnd: HWND) {
    let result = (|| -> Result<String> {
        let config = AppConfig::load()?;
        let (tx, rx) = crossbeam_channel::bounded::<Vec<i16>>(512);
        let _capture =
            crate::audio::start_capture(&config.audio_device_contains, config.audio_gain, tx)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut samples = Vec::new();
        while std::time::Instant::now() < deadline {
            if let Ok(frame) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
                samples.extend_from_slice(&frame);
            }
        }
        let engine = crate::parakeet_native::ParakeetNative::load_from_config(&config)?;
        engine.transcribe_pcm16_16k(&samples)
    })();
    match result {
        Ok(text) => {
            if !TEST_OUTPUT.is_null() {
                SetWindowTextW(TEST_OUTPUT, wide_null(&text).as_ptr());
            }
        }
        Err(error) => info(hwnd, &format!("Record test failed:\n{error:#}")),
    }
}

unsafe fn test_model(hwnd: HWND) {
    match AppConfig::load().and_then(|config| {
        let audio = crate::config::repo_root()
            .join("tests")
            .join("fixtures")
            .join("parakeet-smoke.wav");
        models::smoke_test_model(
            &config.parakeet_runtime_dir_path(),
            &config.parakeet_model_path(),
            &audio,
        )
    }) {
        Ok(text) => info(hwnd, &format!("Model test passed:\n{text}")),
        Err(error) => info(hwnd, &format!("Model test failed:\n{error:#}")),
    }
}

unsafe fn download_recommended(hwnd: HWND) {
    let result = (|| -> Result<String> {
        let mut config = AppConfig::load()?;
        let selected = selected_model_file().unwrap_or_else(|| "tdt_ctc-110m-f16.gguf".to_owned());
        let path = models::download_model(&selected)?;
        let audio = crate::config::repo_root()
            .join("tests")
            .join("fixtures")
            .join("parakeet-smoke.wav");
        let text = models::smoke_test_model(&config.parakeet_runtime_dir_path(), &path, &audio)?;
        config.parakeet_model_path = path.to_string_lossy().into_owned();
        config.save()?;
        Ok(text)
    })();
    match result {
        Ok(text) => info(
            hwnd,
            &format!("Downloaded, tested, and selected recommended model:\n{text}"),
        ),
        Err(error) => info(hwnd, &format!("Download/test failed:\n{error:#}")),
    }
}

unsafe fn output_edit(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32) {
    TEST_OUTPUT = CreateWindowExW(
        WS_BORDER,
        wide_null("EDIT").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | 0x0004 | 0x0040 | 0x00200000,
        x,
        y,
        w,
        h,
        hwnd,
        null_mut(),
        null_mut(),
        null_mut(),
    );
}

unsafe fn combo(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    let combo = CreateWindowExW(
        0,
        wide_null("COMBOBOX").as_ptr(),
        null_mut(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | 0x0003,
        x,
        y,
        w,
        h,
        hwnd,
        ID_MODEL_COMBO as _,
        null_mut(),
        null_mut(),
    );
    MODEL_COMBO = combo;
    let mut selected = 0_usize;
    for (idx, model) in models::catalog().iter().enumerate() {
        let suffix = if model.recommended {
            " recommended"
        } else {
            ""
        };
        let label = format!(
            "{} | {} | {} MB{}",
            model.family, model.quant, model.size_mb, suffix
        );
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            combo,
            CB_ADDSTRING,
            0,
            wide_null(&label).as_ptr() as isize,
        );
        if model.file == "tdt_ctc-110m-f16.gguf" {
            selected = idx;
        }
    }
    windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(combo, CB_SETCURSEL, selected, 0);
}

unsafe fn selected_model_file() -> Option<String> {
    if MODEL_COMBO.is_null() {
        return None;
    }
    let index =
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(MODEL_COMBO, CB_GETCURSEL, 0, 0);
    if index < 0 {
        return None;
    }
    let mut buf = [0_u16; 256];
    windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
        MODEL_COMBO,
        CB_GETLBTEXT,
        index as usize,
        buf.as_mut_ptr() as isize,
    );
    let nul = buf
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buf.len());
    let label = String::from_utf16_lossy(&buf[..nul]);
    let mut parts = label.split('|').map(str::trim);
    let family = parts.next()?;
    let quant = parts.next()?;
    Some(format!("{family}-{quant}.gguf"))
}

unsafe fn label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32) {
    CreateWindowExW(
        0,
        wide_null("STATIC").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        w,
        h,
        hwnd,
        null_mut(),
        null_mut(),
        null_mut(),
    );
}

unsafe fn edit(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32) {
    CreateWindowExW(
        WS_BORDER,
        wide_null("EDIT").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        x,
        y,
        w,
        h,
        hwnd,
        null_mut(),
        null_mut(),
        null_mut(),
    );
}

unsafe fn button(hwnd: HWND, text: &str, id: usize, x: i32, y: i32, w: i32, h: i32) {
    CreateWindowExW(
        0,
        wide_null("BUTTON").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        x,
        y,
        w,
        h,
        hwnd,
        id as _,
        null_mut(),
        null_mut(),
    );
}

unsafe fn info(hwnd: HWND, message: &str) {
    MessageBoxW(
        hwnd,
        wide_null(message).as_ptr(),
        wide_null("Uvox").as_ptr(),
        MB_OK | MB_ICONINFORMATION,
    );
}
