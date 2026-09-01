//! End-to-end test for the Linux overlay against the live Wayland compositor.
//!
//! Drives a real `OverlayHandle` through show -> hide -> show and screenshots
//! the screen with `grim` at each step so the map/unmap/remap behaviour can be
//! inspected. Run inside a Wayland session: `cargo run --bin overlay-e2e`.

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
    let _ = std::fs::create_dir_all("/tmp/overlay_e2e");
    let path = format!("/tmp/overlay_e2e/{name}.png");
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
}

#[cfg(target_os = "linux")]
fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(target_os = "linux")]
fn main() {
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
    println!("done");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("overlay-e2e is a Linux Wayland-only helper; skipping on this platform");
}
