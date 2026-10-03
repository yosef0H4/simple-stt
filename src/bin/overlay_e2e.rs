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
        let cases = [
            ("loading", OverlayPrimary::Hidden, translate(locale, "notice.modelLoading", &[])),
            ("warming", OverlayPrimary::Hidden, translate(locale, "notice.modelWarming", &[])),
            ("ready", OverlayPrimary::Hidden, translate(locale, "notice.modelReady", &[])),
            ("unloaded", OverlayPrimary::Hidden, translate(locale, "notice.modelUnloaded", &[])),
            ("typing", OverlayPrimary::Typing, String::new()),
            ("mixed-transcribing", OverlayPrimary::Transcribing, "النموذج: \u{2066}lemura-arabic-q8_0.gguf\u{2069}".into()),
            ("recording-warning", OverlayPrimary::Recording, translate(locale, "notice.microphoneFallback", &[])),
            ("multiline-warning", OverlayPrimary::Hidden, "تعذّر الاتصال بخدمة الصوت.\nتحقق من الميكروفون وأعد المحاولة.\nالملف: \u{2066}/home/user/models/arabic-q8.gguf\u{2069}".into()),
            ("wrapped-warning", OverlayPrimary::Recording, translate(locale, "notice.cleanupTimeout", &[("seconds", "120")])),
            ("long-mixed-warning", OverlayPrimary::Recording, translate(locale, "error.missingBinary", &[("binary", "\u{2066}/home/user/simple-stt/runtime/arabic-models/lemura-arabic-asr-lite-q8_0.gguf\u{2069}")])),
        ];
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
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("overlay-e2e is a Linux Wayland-only helper; skipping on this platform");
}
