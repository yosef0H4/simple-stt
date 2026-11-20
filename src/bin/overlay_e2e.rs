//! End-to-end test for the Linux overlay against the live Wayland compositor.
//!
//! Drives a real `OverlayHandle` through show -> hide -> show and screenshots
//! the screen with Spectacle at each step so the map/unmap/remap behaviour can be
//! inspected. Run inside a Wayland session: `cargo run --bin overlay-e2e`.
//! Use `--offscreen` to render the same Pango/Cairo text without touching the desktop.

#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use simple_stt::capture::overlay::OverlayHandle;

#[cfg(target_os = "linux")]
fn shot(name: &str) {
    let directory = std::env::var("SIMPLE_STT_OVERLAY_SCREENSHOT_DIR")
        .unwrap_or_else(|_| "artifacts/localization/native".into());
    std::fs::create_dir_all(&directory).expect("create screenshot directory");
    let path = format!("{directory}/{name}.png");
    let _ = std::fs::remove_file(&path);
    // KWin: spectacle in background, fullscreen, no notification.
    let status = Command::new("spectacle")
        .args(["-b", "-n", "-f", "-o", &path])
        .status();
    // spectacle returns before the file is fully written sometimes; settle.
    std::thread::sleep(Duration::from_millis(400));
    let ok = std::fs::metadata(&path)
        .map(|m| m.len() > 0)
        .unwrap_or(false);
    println!("  shot {name} -> {path} (status={status:?}, written={ok})");
    assert!(
        status.is_ok_and(|status| status.success()) && ok,
        "live screenshot failed"
    );
}

#[cfg(target_os = "linux")]
fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(target_os = "linux")]
fn main() {
    if std::env::args().any(|arg| arg == "--offscreen") {
        render_offscreen();
        return;
    }
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();

    let overlay = OverlayHandle::spawn().expect("spawn overlay");
    let level = overlay.level_cell();
    sleep(500); // let the thread connect and the layer surface settle

    println!("step 1: show");
    overlay.start_recording(0, Default::default());
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("1_show");

    println!("step 2: hide");
    overlay.hide();
    sleep(700);
    shot("2_hidden");

    println!("step 3: show again (the regression)");
    overlay.start_recording(0, Default::default());
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("3_show_again");

    println!("step 4: hide again");
    overlay.hide();
    sleep(500);
    shot("4_hidden_again");

    println!("step 5: third show");
    overlay.start_recording(0, Default::default());
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("5_show_third");

    println!("step 6: resize from visualizer to a long notice");
    overlay.notify_warning(
        "🎙 Preferred microphone unavailable — using system default",
        Duration::from_secs(5),
    );
    sleep(700);
    shot("6_long_notice");

    println!("step 7: clear notice without disturbing the visualizer");
    overlay.clear_notice();
    sleep(700);
    shot("7_visualizer_after_notice");

    overlay.hide();
    sleep(300);
    use simple_stt::localization::{translate, Locale};
    overlay.set_locale(Locale::Ar);
    for (name, id) in [
        ("ar-loading", "notice.modelLoading"),
        ("ar-warming", "notice.modelWarming"),
        ("ar-ready", "notice.modelReady"),
        ("ar-unloaded", "notice.modelUnloaded"),
    ] {
        overlay.clear_notice();
        overlay.notify_info(translate(Locale::Ar, id, &[]), Some(Duration::from_secs(5)));
        sleep(700);
        shot(name);
    }
    overlay.clear_notice();
    overlay.set_primary(simple_stt::capture::overlay::OverlayPrimary::Transcribing);
    overlay.notify_info(
        "النموذج: \u{2066}lemura-arabic-q8_0.gguf\u{2069}",
        Some(Duration::from_secs(5)),
    );
    sleep(700);
    shot("ar-mixed-transcribing");
    overlay.start_recording(0, Default::default());
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    overlay.notify_warning(
        translate(Locale::Ar, "notice.microphoneFallback", &[]),
        Duration::from_secs(5),
    );
    sleep(700);
    shot("ar-recording-warning");
    overlay.hide();
    overlay.notify_warning("تعذّر الاتصال بخدمة الصوت.\nتحقق من الميكروفون وأعد المحاولة.\nالملف: \u{2066}/home/user/models/arabic-q8.gguf\u{2069}", Duration::from_secs(5));
    sleep(700);
    shot("ar-multiline-warning");
    overlay.hide();
    sleep(300);
    println!("done");
}

#[cfg(target_os = "linux")]
fn render_offscreen() {
    use simple_stt::capture::overlay::{overlay_render, OverlayPrimary, RecordingIndicators};
    use simple_stt::localization::{translate, Locale};
    let directory = std::env::var("SIMPLE_STT_OVERLAY_SCREENSHOT_DIR")
        .unwrap_or_else(|_| "artifacts/localization/offscreen".into());
    std::fs::create_dir_all(&directory).expect("create screenshot directory");
    for locale in [Locale::En, Locale::Ar] {
        let activity =
            |value: serde_json::Value| simple_stt::reader::activity_text(&value, locale, false);
        let event = |feature: &str, model: &str, state: &str| {
            simple_stt::reader::model_event_text(
                &serde_json::json!({"feature": feature, "model": model, "language": if feature == "tts" { "en" } else { "ar" }, "event": state}),
                locale,
            )
        };
        let mut reader_cases = vec![
            (
                "read-along-next-page",
                simple_stt::reader::hud_text(&serde_json::json!({"state":"reading","activity":{"phase":"idle","message_id":"idle"},"chunk_text":if locale == Locale::Ar { "هذا هو النص الذي يُقرأ الآن." } else { "This is the sentence being read aloud." },"read_along_active":true,"next_page_activity":{"phase":"ocr"}}),locale,0).0,
            ),
            (
                "reader-detect-language",
                activity(
                    serde_json::json!({"activity": {"phase": "detecting_language", "message_id": "reader.phase.detecting_language"}}),
                ),
            ),
            (
                "reader-loading-voice",
                activity(
                    serde_json::json!({"activity": {"phase": "loading_voice", "message_id": "reader.phase.loading_voice", "model": "kokoro", "language": "en"}}),
                ),
            ),
            (
                "reader-reading",
                activity(
                    serde_json::json!({"activity": {"phase": "reading", "message_id": "reader.phase.reading", "model": "supertonic3", "language": "ar"}, "device": "CUDA verified", "chunk": 1, "chunk_total": 4, "chunk_text": if locale == Locale::Ar { "هذا النص هو ما يُقرأ الآن. السعر ٥ دولارات." } else { "This is the passage being read aloud. The price is five dollars." }}),
                ),
            ),
            (
                "reader-reading-copy-notice",
                simple_stt::reader::hud_text(&serde_json::json!({"state":"reading","activity":{"phase":"reading","message_id":"reader.phase.reading"},"chunk_text":if locale == Locale::Ar { "هذا النص هو ما يُقرأ الآن." } else { "This passage remains visible alongside the clipboard notice." },"notice_text":"reader.clipboard_replaced"}),locale,0).0,
            ),
            (
                "reader-prewarm",
                format!(
                    "{}\n{}",
                    event("ocr", "paddle-ar-en", "loading"),
                    event("tts", "kokoro", "loading")
                ),
            ),
            (
                "reader-loaded",
                format!(
                    "{}\n{}",
                    event("ocr", "paddle-ar-en", "loaded"),
                    event("tts", "kokoro", "loaded")
                ),
            ),
            (
                "reader-unloaded",
                format!(
                    "{}\n{}",
                    event("ocr", "paddle-ar-en", "unloaded"),
                    event("tts", "kokoro", "unloaded")
                ),
            ),
            (
                "reader-clipboard-replaced",
                format!("📋 {}", translate(locale, "reader.clipboard_replaced", &[])),
            ),
        ];
        // Feed actual isolated OCR output through the production HUD and Pango
        // renderer, without mapping a window or touching the clipboard.
        let arguments: Vec<_> = std::env::args().collect();
        if let Some(pair) = arguments
            .windows(2)
            .find(|pair| pair[0] == "--reading-text-file")
        {
            let text = std::fs::read_to_string(&pair[1]).expect("read OCR fixture text");
            reader_cases.push((
                "reader-ocr-result",
                activity(serde_json::json!({
                    "activity": {"phase": "reading", "message_id": "reader.phase.reading", "language": "ar"},
                    "chunk": 1, "chunk_total": 3, "chunk_text": text
                })),
            ));
        }
        let cases = [
            ("compact-assigned", OverlayPrimary::Recording, simple_stt::shortcuts::summary(&std::collections::BTreeMap::from([
                ("record".into(), "Meta+Z".into()),
                ("cancel".into(), "Meta+Ctrl+Z".into()),
                ("delivery".into(), "Meta+Ctrl+.".into()),
                ("retry".into(), "Meta+Ctrl+,".into()),
            ]), locale)),
            ("clipboard-buffering", OverlayPrimary::Hidden, translate(locale, "notice.clipboardBuffering", &[])),
            ("clipboard-ready", OverlayPrimary::Hidden, translate(locale, "notice.clipboardReady", &[])),
            ("clipboard-failed", OverlayPrimary::Hidden, translate(locale, "notice.clipboardFailed", &[])),
            ("record-release", OverlayPrimary::Hidden, translate(locale, "notice.recordRelease", &[])),
            ("retry-release", OverlayPrimary::Hidden, translate(locale, "notice.retryRelease", &[])),
            ("retrying", OverlayPrimary::Hidden, translate(locale, "notice.retrying", &[])),
            ("retry-sent", OverlayPrimary::Hidden, translate(locale, "notice.retrySent", &[])),
            ("retry-empty", OverlayPrimary::Hidden, translate(locale, "notice.retryEmpty", &[])),
            ("retry-failed", OverlayPrimary::Hidden, translate(locale, "notice.retryFailed", &[])),
            ("loading", OverlayPrimary::Hidden, translate(locale, "notice.modelLoading", &[])),
            ("warming", OverlayPrimary::Hidden, translate(locale, "notice.modelWarming", &[])),
            ("ready", OverlayPrimary::Hidden, translate(locale, "notice.modelReady", &[])),
            ("unloaded", OverlayPrimary::Hidden, translate(locale, "notice.modelUnloaded", &[])),
            ("typing", OverlayPrimary::Typing, String::new()),
            ("cleaning", OverlayPrimary::Cleaning, String::new()),
            ("preparing-clipboard", OverlayPrimary::PreparingClipboard, String::new()),
            ("pasting", OverlayPrimary::Pasting, String::new()),
            ("waiting-paste", OverlayPrimary::WaitingForPaste, String::new()),
            ("waiting-keys", OverlayPrimary::WaitingForKeys, String::new()),
            ("mixed-transcribing", OverlayPrimary::Transcribing, "النموذج: \u{2066}lemura-arabic-q8_0.gguf\u{2069}".into()),
            ("recording-countdown-500", OverlayPrimary::Recording, translate(locale, "notice.recordingCountdownMs", &[("milliseconds", "500")])),
            ("recording-countdown-250", OverlayPrimary::Recording, translate(locale, "notice.recordingCountdownMs", &[("milliseconds", "250")])),
            ("recording-warning", OverlayPrimary::Recording, translate(locale, "notice.microphoneFallback", &[])),
            ("multiline-warning", OverlayPrimary::Hidden, "تعذّر الاتصال بخدمة الصوت.\nتحقق من الميكروفون وأعد المحاولة.\nالملف: \u{2066}/home/user/models/arabic-q8.gguf\u{2069}".into()),
            ("wrapped-warning", OverlayPrimary::Recording, translate(locale, "notice.cleanupTimeout", &[("seconds", "120")])),
            ("long-mixed-warning", OverlayPrimary::Recording, translate(locale, "error.missingBinary", &[("binary", "\u{2066}/home/user/simple-stt/runtime/arabic-models/lemura-arabic-asr-lite-q8_0.gguf\u{2069}")])),
        ];
        let bindings = [
            ("record", "Meta+Z"),
            ("cancel", "Meta+X"),
            ("delivery", "Meta+D"),
            ("cleanup", "Meta+Ctrl+A"),
            ("retry", "Meta+Ctrl+V"),
            ("shortcuts", "Meta+Ctrl+H"),
        ]
        .into_iter()
        .map(|(id, chord)| (id.to_owned(), chord.to_owned()))
        .collect();
        let shortcuts = simple_stt::shortcuts::summary(&bindings, locale);
        for (name, primary, notice, enabled) in [
            (
                "reminders-recording",
                OverlayPrimary::Recording,
                String::new(),
                true,
            ),
            (
                "reminders-transcribing",
                OverlayPrimary::Transcribing,
                String::new(),
                true,
            ),
            (
                "reminders-pasting",
                OverlayPrimary::Pasting,
                String::new(),
                true,
            ),
            (
                "reminders-typing",
                OverlayPrimary::Typing,
                String::new(),
                true,
            ),
            (
                "reminders-warning",
                OverlayPrimary::Recording,
                translate(locale, "notice.microphoneFallback", &[]),
                true,
            ),
            (
                "reminders-off",
                OverlayPrimary::Pasting,
                String::new(),
                false,
            ),
            (
                "shortcuts-manual",
                OverlayPrimary::Hidden,
                format!("{}\n{shortcuts}", translate(locale, "shortcut.title", &[])),
                false,
            ),
        ] {
            let plan = overlay_render::plan_with_shortcuts(
                primary,
                Some(&notice),
                &[0.6; 10],
                RecordingIndicators::default(),
                locale,
                &shortcuts,
                enabled,
            )
            .expect("shortcut layout");
            if enabled {
                assert!(plan.text.contains(&shortcuts));
            }
            assert!(
                plan.height < overlay_render::MAX_HEIGHT,
                "shortcut reminder clipped"
            );
            let surface = overlay_render::render_surface(&plan).expect("shortcut surface");
            let mut file =
                std::fs::File::create(format!("{directory}/{}-{name}.png", locale.as_str()))
                    .unwrap();
            surface.write_to_png(&mut file).unwrap();
        }
        for (name, primary, notice) in cases {
            let plan = overlay_render::plan_for_locale(
                primary,
                Some(&notice),
                &[0.6; 10],
                RecordingIndicators::default(),
                locale,
            )
            .expect("layout plan");
            let surface = overlay_render::render_surface(&plan).expect("render tooltip");
            let path = format!("{directory}/{}-{name}.png", locale.as_str());
            let mut file = std::fs::File::create(&path).expect("create PNG");
            surface.write_to_png(&mut file).expect("write PNG");
            println!("{path}: {}×{}", plan.width, plan.height);
        }
        for (name, notice) in reader_cases {
            let plan = overlay_render::plan_for_locale(
                OverlayPrimary::Hidden,
                Some(&notice),
                &[0.6; 10],
                RecordingIndicators::default(),
                locale,
            )
            .expect("reader layout plan");
            let surface = overlay_render::render_surface(&plan).expect("reader tooltip");
            let path = format!("{directory}/{}-{name}.png", locale.as_str());
            let mut file = std::fs::File::create(&path).expect("create PNG");
            surface.write_to_png(&mut file).expect("write PNG");
            println!("{path}: {}×{}", plan.width, plan.height);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    use simple_stt::capture::overlay::{OverlayHandle, OverlayPrimary};
    use std::time::Duration;
    let overlay = OverlayHandle::spawn().expect("spawn native tooltip");
    let locale = if std::env::args().any(|arg| arg == "--arabic") {
        simple_stt::localization::Locale::Ar
    } else {
        simple_stt::localization::Locale::En
    };
    overlay.set_locale(locale);
    if std::env::args().any(|arg| arg == "--reader") {
        use simple_stt::{localization::Locale, reader, shortcuts};
        let passage = if locale == Locale::Ar {
            "هذا النص هو ما يُقرأ الآن. اسم البرنامج Simple STT."
        } else {
            "Holmes is known for his proficiency with observation and deduction."
        };
        let status = serde_json::json!({
            "state": "reading", "chunk_text": passage, "chunk": 2, "chunk_total": 3,
            "read_along_active": true, "next_page_activity": {"phase": "ready"}
        });
        overlay.notify_info(reader::hud_text(&status, locale, 0).0, None);
        std::thread::sleep(Duration::from_secs(3));
        overlay.notify_info(
            shortcuts::summary(
                &[
                    ("record".to_owned(), "CapsLock+S".to_owned()),
                    ("cancel".to_owned(), "CapsLock+A".to_owned()),
                    ("copy-region".to_owned(), "CapsLock+Z".to_owned()),
                ]
                .into(),
                locale,
            ),
            None,
        );
        std::thread::sleep(Duration::from_secs(3));
        let all = shortcuts::ACTIONS
            .into_iter()
            .zip('A'..='K')
            .map(|((id, _), key)| (id.to_owned(), format!("Ctrl+Alt+{key}")))
            .collect();
        overlay.notify_info(shortcuts::summary(&all, locale), None);
        std::thread::sleep(Duration::from_secs(3));
        overlay.hide();
        return;
    }
    let config = simple_stt::config::AppConfig::default();
    overlay.set_shortcuts(simple_stt::shortcuts::current(&config, locale), true);
    overlay.start_recording(0, Default::default());
    println!("recording");
    for index in 0..150 {
        overlay.set_level((index % 20) as f32 / 20.0);
        std::thread::sleep(Duration::from_millis(33));
    }
    overlay.set_primary(OverlayPrimary::Transcribing);
    println!("transcribing");
    std::thread::sleep(Duration::from_secs(2));
    overlay.hide();
    println!("hidden");
    std::thread::sleep(Duration::from_secs(1));
    overlay.start_recording(0, Default::default());
    println!("reopened");
    std::thread::sleep(Duration::from_secs(2));
    let latencies = overlay.benchmark_latency(100);
    assert_eq!(latencies.len(), 100);
    println!(
        "max_command_us={}",
        latencies.iter().max().unwrap().as_micros()
    );
    overlay.hide();
    std::thread::sleep(Duration::from_millis(100));
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    println!("overlay-e2e is a Linux Wayland-only helper; skipping on this platform");
}
