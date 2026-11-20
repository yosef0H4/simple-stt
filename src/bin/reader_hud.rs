//! Reading uses the same native overlay and fonts as dictation.
use anyhow::Result;
use simple_stt::capture::overlay::OverlayHandle;
use std::time::{Duration, Instant};
fn main() -> Result<()> {
    #[cfg(windows)]
    let overlay = OverlayHandle::spawn_reader()?;
    #[cfg(not(windows))]
    let overlay = OverlayHandle::spawn()?;
    let config = simple_stt::config::AppConfig::load()?;
    overlay.set_locale(simple_stt::localization::resolve(
        config.general.ui_language,
    ));
    overlay.set_shortcuts(
        simple_stt::shortcuts::summary(
            &simple_stt::shortcuts::assigned(&config),
            simple_stt::localization::resolve(config.general.ui_language),
        ),
        config.general.shortcut_reminders,
    );
    let locale = simple_stt::localization::resolve(config.general.ui_language);
    // This local progress HUD shows the current passage, as requested. The
    // separate system-notification privacy policy still hides source text.
    let started = Instant::now();
    let mut previous = String::new();
    let mut idle = Instant::now();
    let mut hints_enabled = config.general.shortcut_reminders;
    while let Some(status) = simple_stt::reader::status() {
        let next_hints_enabled =
            simple_stt::reader::shortcut_hints_enabled(&status, config.general.shortcut_reminders);
        if hints_enabled != next_hints_enabled {
            overlay.set_shortcuts(
                simple_stt::shortcuts::summary(&simple_stt::shortcuts::assigned(&config), locale),
                next_hints_enabled,
            );
            hints_enabled = next_hints_enabled;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let (text, active) = simple_stt::reader::hud_text(&status, locale, now_ms);
        if active {
            idle = Instant::now();
        }
        if text != previous {
            if text.is_empty() {
                overlay.hide();
            } else {
                overlay.notify_info(text.clone(), None);
            }
            previous = text;
        }
        if started.elapsed() > Duration::from_secs(3) && idle.elapsed() > Duration::from_secs(3) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    overlay.hide();
    Ok(())
}
