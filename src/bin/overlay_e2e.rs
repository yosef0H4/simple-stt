//! End-to-end test for the Linux overlay against the live Wayland compositor.
//!
//! Drives a real `OverlayHandle` through show -> hide -> show and screenshots
//! the screen with `grim` at each step so the map/unmap/remap behaviour can be
//! inspected. Run inside a Wayland session: `cargo run --bin overlay-e2e`.

use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Duration;

use simple_stt::capture::overlay::OverlayHandle;

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

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();

    let overlay = OverlayHandle::spawn().expect("spawn overlay");
    let level = overlay.level_cell();
    sleep(500); // let the thread connect and the layer surface settle

    println!("step 1: show");
    overlay.start_recording(0);
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("1_show");

    println!("step 2: hide");
    overlay.hide();
    sleep(700);
    shot("2_hidden");

    println!("step 3: show again (the regression)");
    overlay.start_recording(0);
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("3_show_again");

    println!("step 4: hide again");
    overlay.hide();
    sleep(500);
    shot("4_hidden_again");

    println!("step 5: third show");
    overlay.start_recording(0);
    level.store(0.6f32.to_bits(), Ordering::Relaxed);
    sleep(700);
    shot("5_show_third");

    overlay.hide();
    sleep(300);
    println!("done");
}
