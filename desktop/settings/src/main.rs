#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::Result;
use clap::Parser;
use simple_stt::{
    config::AppConfig,
    settings_server::{self, Args, DesktopSettings},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{webview::NewWindowResponse, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

fn main() {
    if let Err(error) = settings_server::run(Args::parse(), Some(open_window)) {
        eprintln!("simple-stt-settings-window: {error:#}");
        std::process::exit(1);
    }
}

fn open_window(settings: DesktopSettings) -> Result<()> {
    let data_dir = AppConfig::local_data_dir().join("settings-webview");
    let url = settings.url.clone();
    #[cfg(debug_assertions)]
    let url = settings_dev_url(url)?;
    let origin = url.origin();
    tauri::Builder::default()
        .setup(move |app| {
            let loaded = Arc::new(AtomicBool::new(false));
            let page_loaded = loaded.clone();
            let close = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, "settings", WebviewUrl::External(url))
                .title("Simple STT Settings")
                .inner_size(1024.0, 760.0)
                .min_inner_size(320.0, 360.0)
                .decorations(false)
                .data_directory(data_dir)
                .initialization_script(
                    "window.__SIMPLE_STT_DESKTOP__ = true;
                    window.addEventListener('settings-close-approved', (event) => {
                        event.preventDefault();
                        window.location.href = '/__settings_close';
                    });",
                )
                .on_navigation(move |url| {
                    if url.origin() != origin {
                        return false;
                    }
                    if url.path() == "/__settings_close" {
                        // Leave the webview callback before destroying its window.
                        let handle = close.clone();
                        std::thread::spawn(move || {
                            if let Some(window) = handle.get_webview_window("settings") {
                                let _ = window.destroy();
                            }
                        });
                        return false;
                    }
                    let action = match url.path() {
                        "/__settings_frame/drag" => "drag",
                        "/__settings_frame/minimize" => "minimize",
                        "/__settings_frame/maximize" => "maximize",
                        _ => return true,
                    };
                    let handle = close.clone();
                    let _ = close.run_on_main_thread(move || {
                        if let Some(window) = handle.get_webview_window("settings") {
                            if let Err(error) = frame_action(&window, action) {
                                eprintln!("simple-stt-settings-window: {action}: {error}");
                            }
                        }
                    });
                    false
                })
                .on_new_window(|url, _| {
                    // The cleanup OAuth flow uses the user's normal browser session.
                    if url.scheme() == "https" {
                        let _ = open::that_detached(url.as_str());
                    }
                    NewWindowResponse::Deny
                })
                .on_page_load(move |_, payload| {
                    use tauri::webview::PageLoadEvent;
                    page_loaded.store(
                        payload.event() == PageLoadEvent::Finished,
                        Ordering::Relaxed,
                    );
                })
                .build()?;
            let target = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if loaded.load(Ordering::Relaxed)
                        && target
                            .eval("window.dispatchEvent(new Event('settings-close-request'))")
                            .is_ok()
                    {
                        api.prevent_close();
                    }
                }
            });
            let focus_window = window.clone();
            settings.serve(
                move || {
                    let target = focus_window.clone();
                    let _ = focus_window.run_on_main_thread(move || {
                        let _ = target.unminimize();
                        let _ = target.set_focus();
                    });
                },
                move || {
                    let _ = window.destroy();
                },
            );
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run_return(|_, _| {});
    Ok(())
}

fn frame_action(window: &tauri::WebviewWindow, action: &str) -> tauri::Result<()> {
    match action {
        "drag" => window.start_dragging(),
        "minimize" => window.minimize(),
        "maximize" if window.is_maximized()? => window.unmaximize(),
        "maximize" => window.maximize(),
        _ => Ok(()),
    }
}

#[cfg(debug_assertions)]
fn settings_dev_url(url: tauri::Url) -> Result<tauri::Url> {
    let Ok(origin) = std::env::var("SIMPLE_STT_SETTINGS_DEV_ORIGIN") else {
        return Ok(url);
    };
    let mut dev = tauri::Url::parse(&origin)?;
    anyhow::ensure!(
        dev.scheme() == "http"
            && dev.host_str() == Some("127.0.0.1")
            && dev.port().is_some()
            && dev.username().is_empty()
            && dev.password().is_none()
            && dev.path() == "/"
            && dev.query().is_none()
            && dev.fragment().is_none(),
        "SIMPLE_STT_SETTINGS_DEV_ORIGIN must be an HTTP loopback origin with a port"
    );
    dev.set_fragment(url.fragment());
    Ok(dev)
}
