//! Host bridge to the optional reading controller. No inference runs here.
use anyhow::{Context, Result};
use reader_core::{AppConfig, ConfigStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("SIMPLE_STT_READER_DATA") {
        return PathBuf::from(path);
    }
    crate::config::AppConfig::local_data_dir().join("reader")
}
pub fn store() -> ConfigStore {
    ConfigStore::new(data_dir().join("config.json"))
}
pub fn defaults() -> AppConfig {
    let mut config = AppConfig::default();
    config.speech.model_dir = crate::assets::directory("kokoro");
    config.speech.runtime_dir = crate::assets::directory("sherpa-cpu");
    config.ocr.model_dir = crate::assets::root().join("paddle-ar-en");
    config
}
pub fn load() -> Result<AppConfig> {
    if !store().path().exists() {
        return Ok(defaults());
    }
    store().load()
}
pub fn initialize() -> Result<()> {
    if !store().path().exists() {
        let _ = store().save(defaults(), 1)?;
    }
    Ok(())
}
pub fn command(args: &[&str]) -> Result<String> {
    command_input(args, None)
}
fn command_input(args: &[&str], input: Option<&[u8]>) -> Result<String> {
    use std::io::Write;
    let executable = std::env::current_exe()?
        .parent()
        .context("Executable has no directory")?
        .join(if cfg!(windows) {
            "readerctl.exe"
        } else {
            "readerctl"
        });
    anyhow::ensure!(executable.is_file(), "Reading components are not installed");
    initialize()?;
    let mut child = Command::new(executable)
        .args(args)
        .env("SIMPLE_STT_EMBEDDED_READER", "1")
        .env("SIMPLE_STT_READER_DATA", data_dir())
        .env("SIMPLE_TTS_CONFIG", store().path())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .context("Reading stdin unavailable")?
            .write_all(input)?;
    }
    let start = Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Reading command timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
pub fn status() -> Option<Value> {
    #[cfg(unix)]
    {
        let mut stream =
            std::os::unix::net::UnixStream::connect(data_dir().join("simple-tts.sock")).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .ok()?;
        stream
            .set_write_timeout(Some(Duration::from_millis(500)))
            .ok()?;
        reader_core::wire::write_json_frame(&mut stream, &json!({"op":"status"})).ok()?;
        let reply: Value = reader_core::wire::read_json_frame(&mut stream).ok()?;
        reply.get("state").cloned()
    }
    #[cfg(not(unix))]
    {
        command(&["status", "--json"])
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }
}

/// Shared progress suffix for ordinary reading and read-along, including RTL text.
fn append_chunk_counter(line: &mut String, status: &Value) {
    if let Some(total) = status["chunk_total"].as_u64().filter(|total| *total > 0) {
        let index = status["chunk"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(1)
            .min(total);
        line.push_str(&format!(" · \u{2066}{index}/{total}\u{2069}"));
    }
}

/// The HUD renders live backend state. It never queues completed phases merely
/// to keep them visible, and never includes source text when privacy is enabled.
pub fn activity_text(
    status: &Value,
    locale: crate::localization::Locale,
    hide_text: bool,
) -> String {
    let mut lines = Vec::new();
    for name in ["activity", "ocr_activity"] {
        let activity = &status[name];
        let phase = activity["phase"].as_str().unwrap_or("idle");
        if matches!(phase, "idle" | "error") {
            continue;
        }
        let key = activity["message_id"].as_str().unwrap_or("");
        if !crate::localization::contains(key) {
            continue;
        }
        let icon = match phase {
            "detecting_language" => "🌐",
            "recognizing" | "selecting_region" => "🔍",
            "copying" | "acquiring_selection" => "📋",
            "paused" => "⏸️",
            _ => "🔊",
        };
        let mut line = format!(
            "{icon} {}",
            crate::localization::translate(locale, key, &[])
        );
        let spoken = (name == "activity" && matches!(phase, "reading" | "paused") && !hide_text)
            .then(|| status["chunk_text"].as_str())
            .flatten()
            .filter(|text| !text.trim().is_empty());
        if let Some(text) = spoken {
            // Isolate user prose from the overlay's UI direction and progress count.
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut preview: String = normalized.chars().take(240).collect();
            if normalized.chars().count() > 240 {
                preview.push('…');
            }
            line = format!("{icon} \u{2068}{preview}\u{2069}");
        }
        if let Some(language) = activity["language"].as_str().filter(|_| {
            spoken.is_none()
                && !matches!(
                    phase,
                    "detecting_language" | "selecting_region" | "acquiring_selection"
                )
        }) {
            let code = reader_core::routing::primary(language);
            let key = format!("reader.language.{code}");
            let label = if crate::localization::contains(&key) {
                crate::localization::translate(locale, &key, &[])
            } else {
                code
            };
            line.push_str(&format!(" · {label}"));
        }
        if name == "activity" && matches!(phase, "reading" | "playing" | "paused") {
            append_chunk_counter(&mut line, status);
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// Compose the local progress HUD so a transient notice never hides ongoing work.
/// Notification privacy applies to system notices separately from this local HUD.
pub fn hud_text(
    status: &Value,
    locale: crate::localization::Locale,
    now_ms: u64,
) -> (String, bool) {
    let mut lines = Vec::new();
    let activity = activity_text(status, locale, false);
    let current_text = status["chunk_text"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            status["sentence_text"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
        });
    if let Some(current_text) = current_text.filter(|_| {
        matches!(
            status["state"].as_str(),
            Some("reading" | "preparing" | "paused")
        )
    }) {
        let icon = if status["state"] == "paused" {
            "⏸️"
        } else {
            "🔊"
        };
        let mut line = format!(
            "{icon} \u{2068}{}\u{2069}",
            current_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
        append_chunk_counter(&mut line, status);
        lines.push(line);
    } else if !activity.is_empty() {
        lines.push(activity);
    } else if status["read_pending"] == true
        || matches!(status["state"].as_str(), Some("preparing" | "reading"))
    {
        lines.push(format!(
            "🔊 {}",
            crate::localization::translate(locale, "reader.preparing", &[])
        ));
    }
    if status["read_along_active"] == true {
        if let Some(phase) = status["next_page_activity"]["phase"].as_str() {
            let label = match phase {
                "advancing" => "reader.next.advancing",
                "waiting" => "reader.next.waiting",
                "capturing" => "reader.next.capturing",
                "ocr" => "reader.next.ocr",
                "tts" => "reader.next.tts",
                "ready" => "reader.next.ready",
                _ => "reader.next.preparing",
            };
            let mut line = format!("📄 {}", crate::localization::translate(locale, label, &[]));
            if phase == "waiting" {
                let milliseconds = status["next_page_activity"]["wait_remaining_ms"]
                    .as_u64()
                    .unwrap_or(0);
                line.push_str(&format!(" · {:.1}s", milliseconds as f64 / 1000.0));
            }
            lines.push(line);
        }
    }
    if let Some(notice) = status["notice_text"].as_str().filter(|s| !s.is_empty()) {
        if notice == "Read-along waiting for selected window" {
            lines.push(format!(
                "⏳ {}",
                crate::localization::translate(locale, "reader.waitWindow", &[])
            ));
        } else {
            let (icon, key) = match notice {
                "reader.clipboard_replaced" => ("📋", "reader.clipboard_replaced"),
                "Text copied" => ("📋", "reader.copied"),
                value if value.starts_with("Clipboard listening enabled") => {
                    ("📋", "reader.listening")
                }
                value if value.starts_with("Clipboard listening disabled") => {
                    ("📋", "reader.listenerOff")
                }
                value if value.starts_with("Clipboard listener stopped") => {
                    ("⚠️", "reader.listenerStopped")
                }
                _ => ("⚠️", "reader.failed"),
            };
            lines.push(format!(
                "{icon} {}",
                crate::localization::translate(locale, key, &[])
            ));
            if notice.starts_with("Read-along") {
                if let Some((_, detail)) = notice.split_once(": ") {
                    lines.push(detail.chars().take(320).collect());
                }
            }
        }
    }
    let models = model_events_text(status, locale, now_ms);
    if !models.is_empty() {
        lines.push(models);
    }
    let active = !lines.is_empty();
    (lines.join("\n"), active)
}

pub fn shortcut_hints_enabled(status: &Value, configured: bool) -> bool {
    configured && status["read_along_active"] != true
}

pub fn model_event_text(event: &Value, locale: crate::localization::Locale) -> String {
    let feature = event["feature"].as_str().unwrap_or("");
    let state = event["event"].as_str().unwrap_or("");
    let key = format!("reader.model.{feature}.{state}");
    if !crate::localization::contains(&key) {
        return String::new();
    }
    let language = event["language"].as_str().unwrap_or("");
    let code = reader_core::routing::primary(language);
    let language_key = format!("reader.language.{code}");
    let language = if crate::localization::contains(&language_key) {
        crate::localization::translate(locale, &language_key, &[])
    } else {
        String::new()
    };
    crate::localization::translate(locale, &key, &[("language", &language)])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches('·')
        .trim()
        .to_owned()
}

/// Show only the latest lifecycle state for each feature and model.
pub fn model_events_text(
    status: &Value,
    locale: crate::localization::Locale,
    now_ms: u64,
) -> String {
    let mut latest = std::collections::BTreeMap::new();
    if let Some(events) = status["model_events"].as_array() {
        for event in events {
            let key = (
                event["feature"].as_str().unwrap_or(""),
                event["model"].as_str().unwrap_or(""),
            );
            let sequence = event["sequence"].as_u64().unwrap_or(0);
            if latest
                .get(&key)
                .is_none_or(|old: &&Value| old["sequence"].as_u64().unwrap_or(0) <= sequence)
            {
                latest.insert(key, event);
            }
        }
    }
    let mut events = latest.into_values().collect::<Vec<_>>();
    events.sort_by_key(|event| std::cmp::Reverse(event["sequence"].as_u64().unwrap_or(0)));
    events
        .into_iter()
        .filter(|event| {
            let loading = event["event"] == "loading";
            let duplicate_voice = loading
                && event["feature"] == "tts"
                && status["activity"]["phase"] == "loading_voice"
                && event["model"].as_str().is_some_and(|model| {
                    !model.is_empty() && status["activity"]["model"].as_str() == Some(model)
                });
            !duplicate_voice
                && (loading
                    || now_ms.saturating_sub(event["happened_unix_ms"].as_u64().unwrap_or(0))
                        < 3000)
        })
        .map(|event| model_event_text(event, locale))
        .filter(|text| !text.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod hud_formatter_tests {
    use super::{activity_text, model_event_text, model_events_text};
    use crate::localization::Locale;
    use serde_json::json;

    #[test]
    fn latest_model_events_are_distinct_and_do_not_repeat_current_voice_loading() {
        let mut status = json!({
            "activity": {"phase": "loading_voice", "model": "kokoro"},
            "model_events": [
                {"sequence": 1, "happened_unix_ms": 100, "feature": "tts", "model": "kokoro", "event": "loading"},
                {"sequence": 2, "happened_unix_ms": 101, "feature": "ocr", "model": "paddle-ar-en", "language": "ar", "event": "loading"},
                {"sequence": 3, "happened_unix_ms": 102, "feature": "tts", "model": "supertonic3", "language": "en", "event": "loading"}
            ]
        });
        let text = model_events_text(&status, Locale::En, 103);
        assert!(!text.contains("Kokoro"));
        assert!(
            text.contains("English voice loading")
                && text.contains("Arabic text recognition loading")
        );
        assert!(!text.contains("Supertonic") && !text.contains("Paddle"));
        status["model_events"].as_array_mut().unwrap().push(json!(
            {"sequence": 4, "happened_unix_ms": 103, "feature": "ocr", "model": "paddle-ar-en", "language": "ar", "event": "loaded"}
        ));
        let text = model_events_text(&status, Locale::En, 104);
        assert!(text.contains("Arabic text recognition ready"));
        assert!(!text.contains("Arabic text recognition loading"));
    }

    #[test]
    fn lifecycle_notices_use_route_language_and_never_identifiers() {
        let event = json!({"feature": "tts", "model": "secret-model", "runtime": "cuda", "language": "ar-SA", "event": "loaded"});
        assert_eq!(model_event_text(&event, Locale::En), "Arabic voice ready");
        assert_eq!(
            model_event_text(&event, Locale::Ar),
            "العربية · القراءة جاهزة"
        );
        let unknown = json!({"feature": "ocr", "model": "secret-model", "language": "auto", "event": "loading"});
        assert_eq!(
            model_event_text(&unknown, Locale::En),
            "text recognition loading…"
        );
    }

    #[test]
    fn read_along_wait_and_failure_show_the_actual_reason() {
        let waiting = json!({"notice_text":"Read-along waiting for selected window"});
        let text = super::hud_text(&waiting, Locale::En, 0).0;
        assert!(text.contains("Return to reading window"));
        assert!(!text.contains("Reading failed"));
        let failed =
            json!({"notice_text":"Read-along OCR failed: no readable text in the selected region"});
        assert!(super::hud_text(&failed, Locale::En, 0)
            .0
            .contains("no readable text"));
    }

    #[test]
    fn reading_shows_current_prepared_passage_instead_of_generic_phase() {
        let mut status = json!({"activity":{"phase":"reading","message_id":"reader.phase.reading","language":"ar"},"chunk_text":"هذا نص مقروء 123","chunk":0,"chunk_total":2});
        let text = activity_text(&status, Locale::Ar, false);
        assert!(text.contains("\u{2068}هذا نص مقروء 123\u{2069}"));
        assert!(!text.contains(&crate::localization::translate(
            Locale::Ar,
            "reader.phase.reading",
            &[]
        )));
        status["chunk_text"] = json!("The next prepared passage.");
        let next = activity_text(&status, Locale::En, false);
        assert!(next.contains("The next prepared passage."));
        assert!(!next.contains("هذا نص"));
        status["chunk_text"] = json!("");
        assert!(activity_text(&status, Locale::En, false).contains("Reading"));
    }
    #[test]
    fn clipboard_notice_does_not_replace_spoken_passage() {
        let status = json!({"state":"reading","activity":{"phase":"reading","message_id":"reader.phase.reading"},"chunk_text":"Current prepared passage.","notice_text":"reader.clipboard_replaced"});
        let (text, active) = super::hud_text(&status, Locale::En, 0);
        assert!(active);
        assert!(text.contains("Current prepared passage."));
        assert!(text.contains(&crate::localization::translate(
            Locale::En,
            "reader.clipboard_replaced",
            &[]
        )));
        let (text, active) = super::hud_text(&json!({"state":"idle"}), Locale::En, 0);
        assert!(!active && text.is_empty());
    }
    #[test]
    fn normal_and_read_along_huds_share_visible_chunk_progress() {
        for read_along in [false, true] {
            let mut status = json!({"state":"reading","activity":{"phase":"playing","message_id":"idle"},"chunk_text":"Current passage","chunk":0,"chunk_total":6,"read_along_active":read_along,"next_page_activity":{"phase":"ocr"}});
            let first = super::hud_text(&status, Locale::En, 0).0;
            assert!(first.contains("1/6"), "{first}");
            status["chunk"] = json!(4);
            let next = super::hud_text(&status, Locale::Ar, 0).0;
            assert!(next.contains("5/6"), "{next}");
            assert_eq!(next.matches("5/6").count(), 1);
        }
    }
    #[test]
    fn hud_keeps_simple_progress_when_buffer_metrics_are_available() {
        let status = json!({"state":"reading","chunk_text":"Current passage","chunk":0,"chunk_total":6,"read_along_active":true,"chunks_ready_ahead":2,"next_page_activity":{"phase":"ready","generated_chunks":2,"chunk_total":8}});
        let text = super::hud_text(&status, Locale::En, 0).0;
        assert!(text.contains("1/6") && !text.contains("[1/6]"), "{text}");
        assert!(text.contains("Next page · ready"), "{text}");
        assert!(!text.contains("queued") && !text.contains("2/8"), "{text}");
    }
    #[test]
    fn settling_hud_shows_remaining_wait_instead_of_capturing() {
        let status = json!({"state":"reading","chunk_text":"Current passage","chunk":1,"chunk_total":6,"read_along_active":true,"next_page_activity":{"phase":"waiting","wait_remaining_ms":1250}});
        let text = super::hud_text(&status, Locale::En, 0).0;
        assert!(text.contains("2/6") && !text.contains("[2/6]"));
        assert!(text.contains("waiting") && text.contains("1.2s"), "{text}");
        assert!(!text.contains("capturing"));
    }
    #[test]
    fn reading_falls_back_to_current_sentence_when_activity_metadata_is_stale() {
        let status = json!({"state":"reading","activity":{"phase":"idle","message_id":"idle"},"chunk_text":"Audio is playing now.","read_along_active":true,"next_page_activity":{"phase":"ocr"}});
        let (text, active) = super::hud_text(&status, Locale::En, 0);
        assert!(active);
        assert!(text.contains("Audio is playing now."));
        assert!(text.contains("Next page · recognizing text"));
        assert!(!text.contains("Preparing"));
        assert!(!super::shortcut_hints_enabled(&status, true));
        assert!(super::shortcut_hints_enabled(
            &json!({"read_along_active":false}),
            true
        ));
    }
    #[test]
    fn reading_hides_source_text_when_privacy_is_enabled() {
        let status = json!({"activity": {"phase": "reading", "message_id": "reader.phase.reading", "model": "kokoro", "language": "en"}, "device": "CUDA verified", "chunk": 1, "chunk_total": 3, "chunk_text": "private source"});
        let hidden = activity_text(&status, Locale::En, true);
        assert!(!hidden.contains("private source"));
        assert!(hidden.contains("English") && hidden.contains("2/3"));
        assert!(!hidden.contains("Kokoro"));
        assert!(!hidden.contains("GPU") && !hidden.contains("CPU"));
        assert!(activity_text(&status, Locale::En, false).contains("private source"));
    }
}
pub fn pause_for_dictation() -> Result<()> {
    #[cfg(unix)]
    {
        let Ok(mut stream) =
            std::os::unix::net::UnixStream::connect(data_dir().join("simple-tts.sock"))
        else {
            return Ok(());
        };
        stream.set_write_timeout(Some(Duration::from_millis(150)))?;
        reader_core::wire::write_json_frame(&mut stream, &json!({"op":"action","action":"pause"}))?;
    }
    #[cfg(windows)]
    {
        if status().is_some_and(|s| s["state"].as_str() == Some("reading")) {
            let _ = command(&["pause"]);
        }
    }
    Ok(())
}
fn preflight(name: &str) -> Result<bool> {
    use reader_core::routing::{Feature, SelectionMode};
    let config = load()?;
    let features: &[Feature] = match name {
        "copy_region" => &[Feature::Ocr],
        "read_region" => &[Feature::Ocr, Feature::Tts],
        "read_text" | "read_selection" | "read_clipboard" | "listen_clipboard" => &[Feature::Tts],
        _ => return Ok(true),
    };
    let keyboard = if features.iter().any(|f| match f {
        Feature::Tts => config.speech.selection.mode == SelectionMode::FollowKeyboard,
        Feature::Ocr => config.ocr.selection.mode == SelectionMode::FollowKeyboard,
        _ => false,
    }) {
        crate::capture::input_language::active_keyboard_language().ok()
    } else {
        None
    };
    for feature in features {
        let (selection, local) = match feature {
            Feature::Tts => (
                &config.speech.selection,
                matches!(
                    config.speech.provider,
                    reader_core::SpeechProvider::LocalKokoro
                        | reader_core::SpeechProvider::LocalSupertonic
                ),
            ),
            _ => (
                &config.ocr.selection,
                matches!(config.ocr.provider, reader_core::OcrProvider::LocalPaddle),
            ),
        };
        if !local {
            continue;
        }
        if *feature == Feature::Tts && selection.mode == SelectionMode::AutoDetect {
            let ready = selection.languages.values().any(|binding| {
                let mut single = selection.clone();
                single.mode = SelectionMode::SingleModel;
                single.single = binding.clone();
                reader_core::routing::resolve(&single, *feature, None, &crate::assets::root())
                    .is_ok()
            });
            if ready {
                continue;
            }
        }
        if reader_core::routing::resolve(
            selection,
            *feature,
            keyboard.as_deref(),
            &crate::assets::root(),
        )
        .is_err()
        {
            let binding = selection.binding(keyboard.as_deref());
            let registry = reader_core::routing::registry();
            let runtime = binding
                .runtime
                .as_ref()
                .and_then(|id| registry.runtimes.iter().find(|r| r.id == *id));
            let runtime_ready = runtime.is_some_and(|r| {
                reader_core::routing::runtime_installed(r, &crate::assets::root())
            });
            let model_ready = binding.model.as_ref().is_some_and(|id| {
                reader_core::routing::asset_installed(id, &crate::assets::root())
                    && registry
                        .models
                        .iter()
                        .find(|model| model.id == *id)
                        .is_some_and(|model| {
                            selection.mode != SelectionMode::FollowKeyboard
                                || keyboard.as_ref().is_some_and(|language| {
                                    model
                                        .languages
                                        .contains(&reader_core::routing::primary(language))
                                })
                        })
            });
            let config = crate::config::AppConfig::load()?;
            crate::localization::set_language(config.general.ui_language);
            let message = crate::modules::requirement_message(
                feature.id(),
                runtime_ready,
                model_ready,
                keyboard.as_deref(),
            );
            if std::env::var_os("SIMPLE_STT_NO_DESKTOP").is_some() {
                anyhow::bail!("{message}");
            }
            let overlay = crate::capture::overlay::OverlayHandle::spawn()?;
            overlay.set_locale(crate::localization::current());
            overlay.notify_warning(message.clone(), Duration::from_secs(6));
            // This disposable helper owns its notice for its visible lifetime.
            std::thread::sleep(Duration::from_secs(6));
            overlay.hide();
            anyhow::bail!("{message}");
        }
    }
    Ok(true)
}
pub fn action(name: &str, text: &str) -> Result<String> {
    preflight(name)?;
    if matches!(
        name,
        "stop" | "pause" | "resume" | "unload" | "shutdown" | "server_stop"
    ) && status().is_none()
    {
        return Ok("Stopped".into());
    }
    anyhow::ensure!(text.len() <= 1024 * 1024, "Reading text exceeds 1 MiB");
    if name == "read_text" {
        return command_input(&["read-text"], Some(text.as_bytes()));
    }
    let args: &[&str] = match name {
        "read_selection" => &["read-selection"],
        "read_clipboard" => &["read-clipboard"],
        "read_region" => &["read-region"],
        "copy_region" => &["copy-region"],
        "listen_clipboard" => &["listen-clipboard"],
        "read_along" => &["read-along"],
        "stop" => &["stop"],
        "pause" => &["pause"],
        "resume" => &["resume"],
        "next" => &["next-sentence"],
        "previous" => &["previous-sentence"],
        "replay" => &["replay-sentence"],
        "repeat" => &["repeat"],
        "unload" => &["unload-now"],
        "shortcuts" => &["configure-shortcuts"],
        "server_start" => &["server-start"],
        "server_stop" => &["server-stop"],
        "shutdown" => &["shutdown"],
        _ => anyhow::bail!("Unknown reading action"),
    };
    command(args)
}
pub fn state() -> Result<Value> {
    let config = load()?;
    Ok(json!({"config":config,"status":status(),
        "stt_key_saved":crate::cleanup::secrets::stt_api_key().ok().flatten().is_some(),
        "speech_key_saved":reader_core::secrets::speech_key().ok().flatten().is_some(),
        "ocr_key_saved":reader_core::secrets::ocr_key().ok().flatten().is_some()}))
}

/// Media operations are clients of existing services and never load native models.
#[derive(Default)]
pub struct ConfiguredBackend {
    transcriber: crate::capture_cli::Transcriber,
}
impl ConfiguredBackend {
    pub fn poll_idle(&mut self) -> Result<()> {
        self.transcriber.shutdown_if_idle()
    }
}
impl crate::cli::Backend for ConfiguredBackend {
    fn transcribe_wav(&mut self, wav: &std::path::Path, language: Option<&str>) -> Result<String> {
        self.transcriber.transcribe(wav, language)
    }
    fn speak_text(&mut self, text: &str, output: Option<&std::path::Path>) -> Result<()> {
        if let Some(output) = output {
            let directory = data_dir().join("cli");
            std::fs::create_dir_all(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
            }
            let input = directory.join(format!("{}.txt", rand::random::<u64>()));
            std::fs::write(&input, text)?;
            let output = if output.is_absolute() {
                output.to_path_buf()
            } else {
                std::env::current_dir()?.join(output)
            };
            let result = local_request(json!({"op":"render_file","input":input,"output":output}));
            let _ = std::fs::remove_file(input);
            result?;
        } else {
            action("read_text", text)?;
        }
        Ok(())
    }
    fn ocr_image(&mut self, image: &std::path::Path) -> Result<String> {
        let path = image.canonicalize()?;
        let response = local_request(json!({"op":"ocr_file","path":path}))?;
        Ok(response["message"]
            .as_str()
            .context("OCR returned no text")?
            .to_owned())
    }
}
fn local_request(request: Value) -> Result<Value> {
    command(&["ensure"])?;
    #[cfg(unix)]
    let mut stream = {
        let stream = std::os::unix::net::UnixStream::connect(data_dir().join("simple-tts.sock"))?;
        stream.set_read_timeout(Some(Duration::from_secs(600)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        stream
    };
    #[cfg(windows)]
    let mut stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(
            r"\\.\pipe\simple-stt-reader-{}",
            std::env::var("USERNAME").unwrap_or_else(|_| "current-user".into())
        ))?;
    reader_core::wire::write_json_frame(&mut stream, &request)?;
    let reply: Value = reader_core::wire::read_json_frame(&mut stream)?;
    anyhow::ensure!(
        reply["ok"].as_bool() == Some(true),
        "{}",
        reply["message"].as_str().unwrap_or("Reader request failed")
    );
    Ok(reply)
}

/// Cancellable local desktop input; deliberately unavailable on the HTTP server.
#[cfg(target_os = "linux")]
pub fn desktop_input(action: Value, current: &impl Fn() -> bool) -> Result<()> {
    anyhow::ensure!(current(), "dictation superseded before input");
    let id = uuid::Uuid::new_v4().to_string();
    let request_id = id.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(local_request(
            json!({"op":"input","id":request_id,"action":action}),
        ));
    });
    loop {
        if !current() {
            std::thread::spawn(move || {
                let _ = local_request(json!({"op":"cancel_input","id":id}));
            });
            anyhow::bail!("dictation superseded; input may be partially delivered");
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => return result.map(|_| ()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => anyhow::bail!("desktop input controller stopped"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn desktop_input(_action: Value, _current: &impl Fn() -> bool) -> Result<()> {
    anyhow::bail!("desktop portal input is only available on Linux")
}

/// Only metadata is shared between dictation and reading; repeat payloads stay
/// in their owning process's RAM.
pub fn reader_action_is_newer(status: Option<&Value>, dictation_at_ms: u64) -> bool {
    status
        .and_then(|value| value["last_action_at_ms"].as_u64())
        .is_some_and(|at| at > dictation_at_ms)
}
pub fn repeat_if_newer(dictation_at_ms: u64) -> Result<bool> {
    if reader_action_is_newer(status().as_ref(), dictation_at_ms) {
        action("repeat", "")?;
        return Ok(true);
    }
    Ok(false)
}
#[cfg(test)]
mod repeat_tests {
    #[test]
    fn repeat_chooses_latest_action_without_loading_a_payload() {
        use serde_json::json;
        assert!(!super::reader_action_is_newer(None, 10));
        assert!(!super::reader_action_is_newer(Some(&json!({})), 10));
        assert!(!super::reader_action_is_newer(
            Some(&json!({"last_action_at_ms":9})),
            10
        ));
        assert!(!super::reader_action_is_newer(
            Some(&json!({"last_action_at_ms":10})),
            10
        ));
        assert!(super::reader_action_is_newer(
            Some(&json!({"last_action_at_ms":11})),
            10
        ));
    }
}
