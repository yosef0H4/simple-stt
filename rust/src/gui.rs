use anyhow::{Context, Result};
use crossbeam_channel::bounded;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use crate::config::{repo_root, AppConfig, LogLevel};
use crate::models;
use crate::parakeet_native::ParakeetNative;
use crate::slint_ui::SettingsWindow;
use crate::startup;

struct ActiveTestRecording {
    _capture: crate::audio::CaptureHandle,
    samples: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
}

pub fn show_settings() -> Result<()> {
    let app = create_settings_window()?;
    app.run().context("running settings window")
}

pub fn show_settings_on_event_loop() -> Result<()> {
    let exe = std::env::current_exe().context("finding current Uvox executable")?;
    std::process::Command::new(exe)
        .arg("settings")
        .spawn()
        .context("starting settings window process")?;
    Ok(())
}

pub fn open_latest_log() -> Result<()> {
    std::process::Command::new("notepad.exe")
        .arg(AppConfig::log_path())
        .spawn()
        .context("opening latest log")?;
    Ok(())
}

pub fn configure_settings_for_screenshot(app: &SettingsWindow, section: Option<&str>) {
    let config = AppConfig::default();
    apply_config(app, &config);
    app.set_section_index(section_index(section));
    app.set_status_title("Ready".into());
    app.set_status_detail("Hold CapsLock to record. Release to transcribe and type.".into());
    app.set_microphone("Default microphone".into());
    app.set_input_level(0.64);
    app.set_progress_text("Model ready".into());
    app.set_test_transcript("Well, I don't wish to see it any more...".into());
    populate_models(app, &config);
}

fn create_settings_window() -> Result<SettingsWindow> {
    let app = SettingsWindow::new().context("creating settings window")?;
    let config = AppConfig::load()?;
    apply_config(&app, &config);
    populate_models(&app, &config);
    wire_callbacks(&app);
    Ok(app)
}

fn apply_config(app: &SettingsWindow, config: &AppConfig) {
    app.set_status_title("Ready".into());
    app.set_status_detail("Hold CapsLock to record. Release to transcribe and type.".into());
    app.set_microphone(microphone_label(config).into());
    app.set_active_model(model_label(config).into());
    app.set_typing_chunk(config.typing_chunk_chars as i32);
    app.set_typing_interval_ms(config.typing_interval_ms as i32);
    app.set_idle_timeout_secs(config.idle_timeout_secs as i32);
    app.set_start_with_windows(config.start_with_windows);
    app.set_hotkey_enabled(config.hotkey_enabled);
    app.set_capslock_always_off(config.capslock_always_off);
    app.set_log_level(log_level_name(&config.log_level).into());

    app.set_parakeet_runtime_dir(config.parakeet_runtime_dir.clone().into());
    app.set_parakeet_model_path(config.parakeet_model_path.clone().into());

    // Enumerate and populate microphones
    let devices = crate::audio::list_input_devices().unwrap_or_default();
    let mic_name = microphone_label(config);
    let mut selected_index = 0;
    let labels = devices
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if name == &mic_name {
                selected_index = i as i32;
            }
            SharedString::from(name.clone())
        })
        .collect::<Vec<_>>();
    app.set_microphone_labels(ModelRc::new(VecModel::from(labels)));
    app.set_selected_microphone_index(selected_index);
}

fn populate_models(app: &SettingsWindow, config: &AppConfig) {
    let catalog = models::catalog();
    let current_model_file = config
        .parakeet_model_path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| config.parakeet_model_path.clone());

    let mut selected_index = 0;
    let labels = catalog
        .iter()
        .map(|model| {
            let recommendation = if model.recommended {
                "recommended"
            } else {
                "optional"
            };
            SharedString::from(format!(
                "{} | {} | {} MB | {}",
                model.family, model.quant, model.size_mb, recommendation
            ))
        })
        .collect::<Vec<_>>();
    let files = catalog
        .iter()
        .enumerate()
        .map(|(i, model)| {
            if model.file == current_model_file {
                selected_index = i as i32;
            }
            SharedString::from(model.file.clone())
        })
        .collect::<Vec<_>>();
    app.set_model_labels(ModelRc::new(VecModel::from(labels)));
    app.set_model_files(ModelRc::new(VecModel::from(files)));
    app.set_selected_model_index(selected_index);
    set_selected_model_label(app);
    update_selected_model_downloaded(app);
}

fn update_selected_model_downloaded(app: &SettingsWindow) {
    let index = app.get_selected_model_index() as usize;
    let files = app.get_model_files();
    if let Some(file) = files.row_data(index) {
        let path = models::local_model_path(&file.to_string());
        app.set_selected_model_downloaded(path.exists());
    }
}

fn wire_callbacks(app: &SettingsWindow) {
    let weak = app.as_weak();
    app.on_save_requested(move || {
        if let Some(app) = weak.upgrade() {
            match save_from_window(&app) {
                Ok(()) => set_status(&app, "Saved", "Settings were written to config.json.", ""),
                Err(error) => set_status(&app, "Save failed", &error.to_string(), ""),
            }
        }
    });

    let weak = app.as_weak();
    app.on_reset_requested(move || {
        if let Some(app) = weak.upgrade() {
            let config = AppConfig::default();
            match config.save() {
                Ok(()) => {
                    let _ = startup::set_start_with_windows(config.start_with_windows);
                    apply_config(&app, &config);
                    populate_models(&app, &config);
                    set_status(&app, "Reset", "Default settings were restored.", "");
                }
                Err(error) => set_status(&app, "Reset failed", &error.to_string(), ""),
            }
        }
    });

    let weak = app.as_weak();
    app.on_open_log_requested(move || {
        if let Some(app) = weak.upgrade() {
            if let Err(error) = open_latest_log() {
                set_status(&app, "Log open failed", &error.to_string(), "");
            }
        }
    });

    let weak = app.as_weak();
    app.on_model_test_requested(move || {
        if let Some(app) = weak.upgrade() {
            run_background(
                &app,
                "Testing model",
                move || {
                    let config = AppConfig::load()?;
                    let audio = fixture_audio();
                    models::smoke_test_model(
                        &config.parakeet_runtime_dir_path(),
                        &config.parakeet_model_path(),
                        &audio,
                    )
                },
                weak.clone(),
            );
        }
    });

    let weak = app.as_weak();
    let active_recording = Arc::new(Mutex::new(None::<ActiveTestRecording>));
    let active_recording_clone = active_recording.clone();

    app.on_record_test_requested(move || {
        if let Some(app) = weak.upgrade() {
            app.set_test_transcript("".into());
            app.set_input_level(0.0);
            app.set_recording_test(true);
            app.set_busy(true);
            set_status(
                &app,
                "Recording test",
                "Recording audio from selected microphone. Click Stop to transcribe.",
                "Recording test",
            );

            let mic_name = app.get_microphone().to_string();
            let audio_device = if mic_name == "Default microphone" {
                String::new()
            } else {
                mic_name
            };

            let (tx, rx) = bounded(4096);
            let config = match AppConfig::load() {
                Ok(c) => c,
                Err(e) => {
                    app.set_recording_test(false);
                    app.set_busy(false);
                    set_status(&app, "Recording failed", &e.to_string(), "");
                    return;
                }
            };
            let capture = match crate::audio::start_capture(&audio_device, config.audio_gain, tx) {
                Ok(cap) => cap,
                Err(e) => {
                    app.set_recording_test(false);
                    app.set_busy(false);
                    set_status(&app, "Recording failed", &e.to_string(), "");
                    return;
                }
            };

            let samples = Arc::new(Mutex::new(Vec::new()));
            let stop_signal = Arc::new(AtomicBool::new(false));

            let samples_thread = samples.clone();
            let stop_signal_thread = stop_signal.clone();
            let weak_thread = weak.clone();

            thread::spawn(move || {
                while !stop_signal_thread.load(Ordering::SeqCst) {
                    if let Ok(frame) = rx.recv_timeout(Duration::from_millis(50)) {
                        let level = visualizer_level(&frame);
                        let _ = slint::invoke_from_event_loop({
                            let weak = weak_thread.clone();
                            move || {
                                if let Some(app) = weak.upgrade() {
                                    app.set_input_level(level);
                                }
                            }
                        });
                        samples_thread.lock().unwrap().extend_from_slice(&frame);
                    }
                }
            });

            let mut act = active_recording_clone.lock().unwrap();
            *act = Some(ActiveTestRecording {
                _capture: capture,
                samples,
                stop_signal,
            });
        }
    });

    let weak = app.as_weak();
    let active_recording_clone2 = active_recording.clone();
    app.on_stop_test_requested(move || {
        if let Some(app) = weak.upgrade() {
            let recording = {
                let mut act = active_recording_clone2.lock().unwrap();
                act.take()
            };

            if let Some(rec) = recording {
                rec.stop_signal.store(true, Ordering::SeqCst);
                app.set_recording_test(false);
                app.set_input_level(0.0);
                set_status(
                    &app,
                    "Transcribing",
                    "Transcribing captured audio with Parakeet...",
                    "Transcribing",
                );

                let weak_transcribe = weak.clone();
                thread::spawn(move || {
                    // Let the thread finish any final reads
                    thread::sleep(Duration::from_millis(100));
                    let samples = rec.samples.lock().unwrap().clone();
                    let result = (|| -> Result<String> {
                        anyhow::ensure!(
                            samples.len() >= 8_000,
                            "record test captured too little audio (need at least 0.5s)"
                        );
                        let config = AppConfig::load()?;
                        let engine = ParakeetNative::load_from_config(&config)?;
                        engine.transcribe_pcm16_16k(&samples)
                    })();

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak_transcribe.upgrade() {
                            app.set_busy(false);
                            match result {
                                Ok(text) => {
                                    app.set_test_transcript(text.into());
                                    set_status(
                                        &app,
                                        "Complete",
                                        "The operation finished successfully.",
                                        "",
                                    );
                                }
                                Err(error) => set_status(&app, "Failed", &error.to_string(), ""),
                            }
                        }
                    });
                });
            } else {
                app.set_recording_test(false);
                app.set_busy(false);
                set_status(&app, "Failed", "No active recording found.", "");
            }
        }
    });

    let weak = app.as_weak();
    app.on_download_requested(move |index| {
        if let Some(app) = weak.upgrade() {
            let files = app.get_model_files();
            let Some(file) = files.row_data(index as usize) else {
                set_status(&app, "Download failed", "No model is selected.", "");
                return;
            };
            let file = file.to_string();
            let weak_done = weak.clone();
            run_background(
                &app,
                "Activating model",
                move || {
                    let trans = download_test_and_select(&file)?;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak_done.upgrade() {
                            if let Ok(config) = AppConfig::load() {
                                app.set_active_model(model_label(&config).into());
                            }
                            update_selected_model_downloaded(&app);
                        }
                    });
                    Ok(trans)
                },
                weak.clone(),
            );
        }
    });

    let weak = app.as_weak();
    app.on_model_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            let count = app.get_model_labels().row_count() as i32;
            if count > 0 {
                let next = (app.get_selected_model_index() + delta).rem_euclid(count);
                app.set_selected_model_index(next);
                set_selected_model_label(&app);
                update_selected_model_downloaded(&app);
            }
        }
    });

    let weak = app.as_weak();
    app.on_microphone_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            let labels = app.get_microphone_labels();
            let count = labels.row_count() as i32;
            if count > 0 {
                let next = (app.get_selected_microphone_index() + delta).rem_euclid(count);
                app.set_selected_microphone_index(next);
                if let Some(name) = labels.row_data(next as usize) {
                    app.set_microphone(name);
                }
            }
        }
    });

    let weak = app.as_weak();
    app.on_typing_chunk_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            app.set_typing_chunk((app.get_typing_chunk() + delta).clamp(1, 20));
        }
    });

    let weak = app.as_weak();
    app.on_typing_interval_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            app.set_typing_interval_ms((app.get_typing_interval_ms() + delta).clamp(0, 200));
        }
    });

    let weak = app.as_weak();
    app.on_idle_timeout_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            app.set_idle_timeout_secs((app.get_idle_timeout_secs() + delta).clamp(10, 3600));
        }
    });

    let weak = app.as_weak();
    app.on_log_level_delta(move |delta| {
        if let Some(app) = weak.upgrade() {
            let levels = ["Minimal", "Normal", "Debug", "Extreme"];
            let current = levels
                .iter()
                .position(|level| *level == app.get_log_level().as_str())
                .unwrap_or(0) as i32;
            let next = (current + delta).rem_euclid(levels.len() as i32) as usize;
            app.set_log_level(levels[next].into());
        }
    });
}

fn run_background(
    app: &SettingsWindow,
    title: &str,
    work: impl FnOnce() -> Result<String> + Send + 'static,
    weak: slint::Weak<SettingsWindow>,
) {
    app.set_busy(true);
    set_status(app, title, "Working in the background.", title);
    thread::spawn(move || {
        let result = work();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = weak.upgrade() {
                app.set_busy(false);
                app.set_progress_text("".into());
                match result {
                    Ok(text) => {
                        app.set_test_transcript(text.clone().into());
                        set_status(&app, "Complete", "The operation finished successfully.", "");
                    }
                    Err(error) => set_status(&app, "Failed", &error.to_string(), ""),
                }
            }
        });
    });
}

fn save_from_window(app: &SettingsWindow) -> Result<()> {
    let mut config = AppConfig::load()?;
    let mic = app.get_microphone().to_string();
    config.audio_device_contains = if mic == "Default microphone" {
        String::new()
    } else {
        mic
    };
    config.typing_chunk_chars = app.get_typing_chunk().max(1) as usize;
    config.typing_interval_ms = app.get_typing_interval_ms().max(0) as u64;
    config.idle_timeout_secs = app.get_idle_timeout_secs().max(1) as u64;
    config.start_with_windows = app.get_start_with_windows();
    config.hotkey_enabled = app.get_hotkey_enabled();
    config.capslock_always_off = app.get_capslock_always_off();
    config.log_level = parse_log_level(&app.get_log_level());

    config.parakeet_runtime_dir = app.get_parakeet_runtime_dir().to_string();
    config.parakeet_model_path = app.get_parakeet_model_path().to_string();

    startup::set_start_with_windows(config.start_with_windows)?;
    config.save()
}

fn download_test_and_select(file: &str) -> Result<String> {
    let model = models::download_model(file)?;
    let mut config = AppConfig::load()?;
    let runtime = config.parakeet_runtime_dir_path();
    let audio = fixture_audio();
    let transcript = models::smoke_test_model(&runtime, &model, &audio)?;
    config.parakeet_model_path = path_for_config(&model);
    config.save()?;
    Ok(transcript)
}

fn set_status(app: &SettingsWindow, title: &str, detail: &str, progress: &str) {
    app.set_status_title(title.into());
    app.set_status_detail(detail.into());
    app.set_progress_text(progress.into());
}

fn set_selected_model_label(app: &SettingsWindow) {
    let labels = app.get_model_labels();
    if let Some(label) = labels.row_data(app.get_selected_model_index().max(0) as usize) {
        app.set_selected_model_label(label);
    }
}

fn fixture_audio() -> std::path::PathBuf {
    repo_root()
        .join("tests")
        .join("fixtures")
        .join("parakeet-smoke.wav")
}

fn path_for_config(path: &Path) -> String {
    let root = repo_root();
    path.strip_prefix(&root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('/', "\\")
}

fn microphone_label(config: &AppConfig) -> String {
    if config.audio_device_contains.trim().is_empty() {
        "Default microphone".to_owned()
    } else {
        config.audio_device_contains.clone()
    }
}

fn model_label(config: &AppConfig) -> String {
    config
        .parakeet_model_path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| config.parakeet_model_path.clone())
}

fn log_level_name(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Minimal => "Minimal",
        LogLevel::Normal => "Normal",
        LogLevel::Debug => "Debug",
        LogLevel::Extreme => "Extreme",
    }
}

fn parse_log_level(value: &SharedString) -> LogLevel {
    match value.as_str() {
        "Normal" => LogLevel::Normal,
        "Debug" => LogLevel::Debug,
        "Extreme" => LogLevel::Extreme,
        _ => LogLevel::Minimal,
    }
}

fn section_index(section: Option<&str>) -> i32 {
    match section.unwrap_or("general").to_ascii_lowercase().as_str() {
        "audio" => 1,
        "model" => 2,
        "typing" => 3,
        "logging" => 4,
        "advanced" => 5,
        _ => 0,
    }
}

fn visualizer_level(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum = frame
        .iter()
        .map(|sample| {
            let value = *sample as f32 / 32768.0;
            value * value
        })
        .sum::<f32>();
    let rms = (sum / frame.len() as f32).sqrt();
    (rms * 14.0).powf(0.72).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_from_window_persists_general_settings() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        std::env::set_var("UVOX_CONFIG", &path);

        let app = SettingsWindow::new().unwrap();
        app.set_microphone("Default microphone".into());
        app.set_typing_chunk(7);
        app.set_typing_interval_ms(55);
        app.set_idle_timeout_secs(240);
        app.set_start_with_windows(true);
        app.set_hotkey_enabled(false);
        app.set_capslock_always_off(true);
        app.set_log_level("Extreme".into());
        app.set_parakeet_runtime_dir("external\\parakeet-runtime\\parakeet-windows-cuda".into());
        app.set_parakeet_model_path(
            "external\\parakeet-runtime\\parakeet-windows-cuda\\models\\tdt_ctc-110m-f16.gguf"
                .into(),
        );

        save_from_window(&app).unwrap();

        let saved = AppConfig::load().unwrap();
        assert_eq!(saved.typing_chunk_chars, 7);
        assert_eq!(saved.typing_interval_ms, 55);
        assert_eq!(saved.idle_timeout_secs, 240);
        assert!(saved.start_with_windows);
        assert!(!saved.hotkey_enabled);
        assert!(saved.capslock_always_off);
        assert_eq!(saved.log_level, LogLevel::Extreme);

        std::env::remove_var("UVOX_CONFIG");
    }

    #[test]
    fn general_toggles_use_two_way_bindings() {
        let source = include_str!("../ui/settings.slint");
        assert!(source.contains("checked <=> root.hotkey-enabled"));
        assert!(source.contains("checked <=> root.start-with-windows"));
        assert!(source.contains("checked <=> root.capslock-always-off"));
    }
}
