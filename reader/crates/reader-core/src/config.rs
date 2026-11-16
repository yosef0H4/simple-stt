use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DevicePreference {
    #[default]
    Auto,
    Cpu,
    Cuda,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpeechProvider {
    #[default]
    LocalKokoro,
    LocalSupertonic,
    OpenAiCompatible {
        base_url: String,
        model: String,
        voice: String,
        format: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OcrProvider {
    #[default]
    LocalPaddle,
    OpenAiCompatible {
        base_url: String,
        model: String,
        api: OcrApi,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OcrApi {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub revision: u64,
    pub speech: SpeechSettings,
    pub ocr: OcrSettings,
    pub playback: PlaybackSettings,
    pub read_along: ReadAlongSettings,
    pub shortcuts: ShortcutSettings,
    pub selection_copy: SelectionCopySettings,
    pub selection_copy_overrides: Vec<SelectionCopyOverride>,
    pub privacy: PrivacySettings,
    pub server: ServerSettings,
    pub request_budgets: crate::request_budget::RequestBudgets,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            revision: 1,
            speech: SpeechSettings::default(),
            ocr: OcrSettings::default(),
            playback: PlaybackSettings::default(),
            read_along: ReadAlongSettings::default(),
            shortcuts: ShortcutSettings::default(),
            selection_copy: SelectionCopySettings::default(),
            selection_copy_overrides: Vec::new(),
            privacy: PrivacySettings::default(),
            server: ServerSettings::default(),
            request_budgets: crate::request_budget::RequestBudgets::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadAlongAction {
    #[default]
    Key,
    Click,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadAlongSettings {
    /// Prefetch the next page while the current page is being spoken.
    pub prefetch: bool,
    /// Key used to advance to the next page in key-action mode.
    pub advance_key: String,
    /// Recorded physical events; absent values retain the existing key assignment.
    pub sequence: Option<crate::recorded_action::RecordedAction>,
    /// How the user triggers a page advance: a key press or a click.
    pub action: ReadAlongAction,
    /// Delay after an advance action before OCR starts, in milliseconds.
    pub delay_ms: u16,
    /// Maximum number of OCR retries after an advance action.
    pub retry_count: u8,
}

impl Default for ReadAlongSettings {
    fn default() -> Self {
        Self {
            prefetch: true,
            advance_key: "Space".into(),
            sequence: None,
            action: ReadAlongAction::Key,
            delay_ms: 900,
            retry_count: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechRouteDefaults {
    pub provider: SpeechProvider,
    pub key_slot: Option<String>,
    pub use_feature_key: bool,
    pub fallback: Option<crate::routing::Connection>,
    pub fallback_to_local: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeechSettings {
    #[serde(skip)]
    pub route_defaults: Option<SpeechRouteDefaults>,
    pub saved_remote: Option<SpeechProvider>,
    pub key_slot: Option<String>,
    pub remote_fallback: Option<crate::routing::Connection>,
    pub use_feature_key: bool,
    pub selection: crate::routing::Selection,
    pub provider: SpeechProvider,
    /// Retry transient remote failures, then use the installed local speech assignment.
    pub fallback_to_local: bool,
    pub voice: String,
    pub language: String,
    pub speed: f32,
    pub device: DevicePreference,
    pub model_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub worker_library: Option<PathBuf>,
}

impl Default for SpeechSettings {
    fn default() -> Self {
        Self {
            route_defaults: None,
            saved_remote: None,
            key_slot: None,
            remote_fallback: None,
            use_feature_key: true,
            selection: crate::routing::Selection {
                mode: crate::routing::SelectionMode::AutoDetect,
                ..Default::default()
            },
            provider: SpeechProvider::default(),
            fallback_to_local: true,
            voice: "af_heart".into(),
            language: "en-us".into(),
            speed: 1.0,
            device: DevicePreference::Auto,
            model_dir: data_dir().join("models/kokoro-multi-lang-v1_0"),
            runtime_dir: data_dir().join("runtime"),
            worker_library: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrSettings {
    pub saved_remote: Option<OcrProvider>,
    pub idle_timeout_secs: u16,
    pub selection: crate::routing::Selection,
    pub provider: OcrProvider,
    /// Retry transient remote failures, then use the installed local OCR assignment.
    pub fallback_to_local: bool,
    pub language: String,
    pub model_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub device: DevicePreference,
    pub worker_library: Option<PathBuf>,
}

impl Default for OcrSettings {
    fn default() -> Self {
        Self {
            saved_remote: None,
            selection: Default::default(),
            idle_timeout_secs: 300,
            provider: OcrProvider::default(),
            fallback_to_local: true,
            language: "eng".into(),
            model_dir: data_dir().join("../assets/paddle-ar-en"),
            runtime_dir: data_dir().join("../assets/sherpa-cpu"),
            device: DevicePreference::Auto,
            worker_library: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    /// Seconds the TTS worker may remain ready after a job.
    pub keep_warm_seconds: u16,
    pub output_device: Option<String>,
    pub max_buffer_seconds: u8,
    pub sentence_gap_ms: u16,
    pub auto_dismiss_seconds: u8,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            keep_warm_seconds: 300,
            output_device: None,
            max_buffer_seconds: 8,
            sentence_gap_ms: 120,
            auto_dismiss_seconds: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutSettings {
    pub read_selection: String,
    pub read_region: String,
    pub read_along: String,
    pub copy_region: String,
    pub listen_clipboard: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            read_selection: "Ctrl+Alt+R".into(),
            read_region: "Ctrl+Alt+O".into(),
            read_along: "None".into(),
            copy_region: "None".into(),
            listen_clipboard: "Ctrl+Alt+C".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionCopyShortcut {
    #[default]
    CtrlInsert,
    CtrlC,
    CtrlShiftC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionCopyBackend {
    #[default]
    Auto,
    Accessibility,
    Native,
    Wtype,
    Ydotool,
    Xdotool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionCopySettings {
    pub backend: SelectionCopyBackend,
    pub shortcut: SelectionCopyShortcut,
    pub allow_clipboard_copy: bool,
    pub show_clipboard_notice: bool,
}

impl Default for SelectionCopySettings {
    fn default() -> Self {
        Self {
            backend: SelectionCopyBackend::Auto,
            shortcut: SelectionCopyShortcut::CtrlInsert,
            allow_clipboard_copy: true,
            show_clipboard_notice: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCopyOverride {
    pub app_id: String,
    pub shortcut: SelectionCopyShortcut,
    #[serde(default)]
    pub backend: Option<SelectionCopyBackend>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacySettings {
    pub hide_text_in_notifications: bool,
    #[serde(default)]
    pub log_content: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            hide_text_in_notifications: true,
            log_content: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub enabled: bool,
    pub listen: String,
    pub allow_remote: bool,
    pub max_clients: u8,
    pub max_text_bytes: usize,
    pub max_image_pixels: u32,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:8765".into(),
            allow_remote: true,
            max_clients: 4,
            max_text_bytes: 1_048_576,
            max_image_pixels: 20_000_000,
        }
    }
}

impl AppConfig {
    pub fn selection_copy_shortcut(&self, app_id: Option<&str>) -> SelectionCopyShortcut {
        app_id
            .and_then(|identity| {
                self.selection_copy_overrides
                    .iter()
                    .find(|rule| rule.app_id.eq_ignore_ascii_case(identity))
            })
            .map(|rule| rule.shortcut)
            .unwrap_or(self.selection_copy.shortcut)
    }

    pub fn selection_copy_backend(&self, app_id: Option<&str>) -> SelectionCopyBackend {
        app_id
            .and_then(|identity| {
                self.selection_copy_overrides
                    .iter()
                    .find(|rule| rule.app_id.eq_ignore_ascii_case(identity))
            })
            .and_then(|rule| rule.backend)
            .unwrap_or(self.selection_copy.backend)
    }

    fn validate_selection_copy_overrides(&self) -> Result<()> {
        if self.selection_copy_overrides.len() > 64 {
            bail!("at most 64 selection copy overrides are allowed");
        }
        let mut seen = std::collections::HashSet::new();
        for rule in &self.selection_copy_overrides {
            if rule.app_id.is_empty()
                || rule.app_id.len() > 128
                || rule.app_id.trim() != rule.app_id
                || rule.app_id.chars().any(char::is_control)
            {
                bail!(
                    "selection copy app identity must contain 1 to 128 bytes without surrounding whitespace or control characters"
                );
            }
            if !seen.insert(rule.app_id.to_ascii_lowercase()) {
                bail!("duplicate selection copy override for {}", rule.app_id);
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.speech
            .selection
            .validate(crate::routing::Feature::Tts)?;
        self.ocr.selection.validate(crate::routing::Feature::Ocr)?;
        self.validate_selection_copy_overrides()?;
        let budgets = self.request_budgets;
        for (name, value, max) in [
            ("connect", budgets.connect, 30),
            ("queue", budgets.queue, 300),
            ("cold model load", budgets.cold_model_load, 600),
            ("warm model load", budgets.warm_model_load, 300),
            ("warm inference", budgets.warm_inference, 600),
            ("stream inactivity", budgets.stream_inactivity, 300),
            ("total", budgets.total, 1800),
        ] {
            if value.as_secs() == 0 || value.subsec_nanos() != 0 || value.as_secs() > max {
                bail!("request {name} timeout must be a whole number of seconds from 1 to {max}");
            }
        }
        if budgets.total < budgets.cold_model_load
            || budgets.total < budgets.warm_model_load
            || budgets.total < budgets.warm_inference
            || budgets.total < budgets.stream_inactivity
        {
            bail!("request total timeout must be at least every individual request timeout");
        }
        if !self.speech.speed.is_finite() || !(0.5..=2.0).contains(&self.speech.speed) {
            bail!("speech speed must be between 0.5 and 2.0");
        }
        if self.playback.keep_warm_seconds > 3600 {
            bail!("keep-warm duration must be at most 3600 seconds");
        }
        if !(1..=8).contains(&self.playback.max_buffer_seconds) {
            bail!("audio buffer must be between one and eight seconds");
        }
        if self.playback.sentence_gap_ms > 2_000 {
            bail!("sentence gap must be at most 2000 milliseconds");
        }
        if self.read_along.delay_ms > 10_000 {
            bail!("read-along delay must be at most 10000 milliseconds");
        }
        if let Some(sequence) = &self.read_along.sequence {
            sequence.validate()?;
        }
        if self.read_along.retry_count > 5 {
            bail!("read-along retries must be at most 5");
        }
        if self.read_along.advance_key.trim().is_empty()
            || self.read_along.advance_key.len() > 32
            || self.read_along.advance_key.chars().any(char::is_control)
        {
            bail!("read-along advance key must contain 1 to 32 printable characters");
        }
        if self.server.max_text_bytes == 0 || self.server.max_text_bytes > 1_048_576 {
            bail!("server text limit must be between 1 byte and 1 MiB");
        }
        if self.server.max_image_pixels == 0 || self.server.max_image_pixels > 20_000_000 {
            bail!("server image limit must be between 1 and 20 megapixels");
        }
        for (name, value) in [
            ("voice", &self.speech.voice),
            ("speech language", &self.speech.language),
            ("OCR language", &self.ocr.language),
        ] {
            if value.trim().is_empty() || value.len() > 128 {
                bail!("{name} must contain 1 to 128 bytes");
            }
        }
        if let Some(slot) = &self.speech.key_slot {
            crate::secrets::validate_slot(slot)?;
        }
        if let Some(fallback) = &self.speech.remote_fallback {
            fallback.validate(crate::routing::Feature::Tts)?;
        }
        if let SpeechProvider::OpenAiCompatible {
            base_url, model, ..
        } = &self.speech.provider
        {
            crate::provider_url::operation(base_url, "audio/speech")?;
            validate_remote(base_url, model)?;
        }
        if let OcrProvider::OpenAiCompatible {
            base_url,
            model,
            api,
        } = &self.ocr.provider
        {
            crate::provider_url::operation(
                base_url,
                if *api == OcrApi::Responses {
                    "responses"
                } else {
                    "chat/completions"
                },
            )?;
            validate_remote(base_url, model)?;
        }
        Ok(())
    }
}

fn validate_remote(base_url: &str, model: &str) -> Result<()> {
    let parsed = crate::provider_url::parse(base_url)?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
        bail!("provider URL must use HTTP or HTTPS and include a host");
    }
    if model.trim().is_empty() || model.len() > 256 {
        bail!("provider model name is invalid");
    }
    Ok(())
}

pub fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("SIMPLE_STT_READER_DATA") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("simple-stt/reader")
}

pub fn config_path() -> PathBuf {
    std::env::var_os("SIMPLE_TTS_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("simple-stt/reader/config.json")
        })
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn load(&self) -> Result<AppConfig> {
        if !self.path.exists() {
            return Ok(AppConfig::default());
        }
        let config: AppConfig =
            serde_json::from_slice(&fs::read(&self.path).context("reading config")?)
                .context("parsing config")?;
        config.validate()?;
        Ok(config)
    }
    pub fn save(&self, mut next: AppConfig, expected_revision: u64) -> Result<AppConfig> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("creating config directory")?;
        }
        let mut lock_name = self
            .path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("config"))
            .to_os_string();
        lock_name.push(".lock");
        let lock_path = self
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(lock_name);
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .context("opening config update lock")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lock.set_permissions(fs::Permissions::from_mode(0o600))
                .context("protecting config update lock")?;
        }
        lock.lock().context("locking config update")?;
        let current = self.load()?;
        if current.revision != expected_revision {
            bail!(
                "configuration revision conflict: expected {expected_revision}, current {}",
                current.revision
            );
        }
        next.revision = current.revision.saturating_add(1);
        next.validate()?;
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let mut file = fs::File::create(&tmp).context("creating config temporary file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .context("protecting config file")?;
        }
        serde_json::to_writer_pretty(&mut file, &next).context("writing config")?;
        file.write_all(b"\n")?;
        file.sync_all().context("syncing config")?;
        fs::rename(&tmp, &self.path).context("committing config")?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_key_assignments_do_not_turn_into_the_default_recording() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["read_along"]["advance_key"] = "Ctrl+PageDown".into();
        value["read_along"]
            .as_object_mut()
            .unwrap()
            .remove("sequence");
        let config: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.read_along.advance_key, "Ctrl+PageDown");
        assert!(config.read_along.sequence.is_none());
        config.validate().unwrap();
    }
    #[test]
    fn voice_retention_defaults_to_five_minutes_and_allows_one_hour() {
        let mut config = AppConfig::default();
        assert_eq!(config.playback.keep_warm_seconds, 300);
        config.playback.keep_warm_seconds = 3600;
        config.validate().unwrap();
        config.playback.keep_warm_seconds = 3601;
        assert!(config.validate().is_err());
    }

    #[test]
    fn remote_fallback_defaults_on_and_request_budgets_round_trip_as_seconds() {
        let config = AppConfig::default();
        assert!(config.speech.fallback_to_local);
        assert!(config.ocr.fallback_to_local);
        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value["request_budgets"]["connect_timeout_secs"], 3);
        assert_eq!(value["request_budgets"]["queue_timeout_secs"], 30);
        assert_eq!(
            value["request_budgets"]["cold_model_load_timeout_secs"],
            180
        );
        assert_eq!(value["request_budgets"]["warm_model_load_timeout_secs"], 30);
        assert_eq!(value["request_budgets"]["warm_inference_timeout_secs"], 60);
        assert_eq!(
            value["request_budgets"]["stream_inactivity_timeout_secs"],
            60
        );
        assert_eq!(value["request_budgets"]["total_timeout_secs"], 300);
        let round_trip: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip.request_budgets, config.request_budgets);
        round_trip.validate().unwrap();
    }

    #[test]
    fn read_along_defaults_are_enabled_only_by_an_explicit_action_and_are_bounded() {
        let mut config = AppConfig::default();
        assert_eq!(config.read_along, ReadAlongSettings::default());
        assert_eq!(config.shortcuts.read_along, "None");
        config.validate().unwrap();
        config.read_along.retry_count = 6;
        assert!(config.validate().is_err());
        config.read_along.retry_count = 5;
        config.read_along.delay_ms = 10_001;
        assert!(config.validate().is_err());
    }

    #[test]
    fn older_reader_config_gets_default_read_along_settings_and_shortcut() {
        let mut old = serde_json::to_value(AppConfig::default()).unwrap();
        old.as_object_mut().unwrap().remove("read_along");
        old["shortcuts"]
            .as_object_mut()
            .unwrap()
            .remove("read_along");
        let migrated: AppConfig = serde_json::from_value(old).unwrap();
        assert_eq!(migrated.read_along, ReadAlongSettings::default());
        assert_eq!(migrated.shortcuts.read_along, "None");
    }

    #[test]
    fn selection_copy_requires_exact_explicit_app_override() {
        let config = AppConfig {
            selection_copy_overrides: vec![
                SelectionCopyOverride {
                    backend: None,
                    app_id: "Konsole".into(),
                    shortcut: SelectionCopyShortcut::CtrlShiftC,
                },
                SelectionCopyOverride {
                    backend: None,
                    app_id: "Editor".into(),
                    shortcut: SelectionCopyShortcut::CtrlC,
                },
            ],
            ..AppConfig::default()
        };
        config.validate().unwrap();
        assert_eq!(
            config.selection_copy_shortcut(Some("konsole")),
            SelectionCopyShortcut::CtrlShiftC
        );
        assert_eq!(
            config.selection_copy_shortcut(Some("Editor")),
            SelectionCopyShortcut::CtrlC
        );
        for identity in [None, Some("Konsole child"), Some("unknown terminal")] {
            assert_eq!(
                config.selection_copy_shortcut(identity),
                SelectionCopyShortcut::CtrlInsert
            );
        }
        let round_trip: AppConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(
            round_trip.selection_copy_overrides,
            config.selection_copy_overrides
        );
    }

    #[test]
    fn selection_copy_rejects_ambiguous_or_invalid_rules() {
        let mut config = AppConfig::default();
        for identity in [
            "",
            " leading",
            "trailing ",
            "bad\nidentity",
            "bad\0identity",
        ] {
            config.selection_copy_overrides = vec![SelectionCopyOverride {
                backend: None,
                app_id: identity.into(),
                shortcut: SelectionCopyShortcut::CtrlC,
            }];
            assert!(config.validate().is_err());
        }
        config.selection_copy_overrides = vec![
            SelectionCopyOverride {
                backend: None,
                app_id: "Editor".into(),
                shortcut: SelectionCopyShortcut::CtrlC,
            },
            SelectionCopyOverride {
                backend: None,
                app_id: "editor".into(),
                shortcut: SelectionCopyShortcut::CtrlShiftC,
            },
        ];
        assert!(config.validate().is_err());
        let mut legacy = serde_json::to_value(AppConfig::default()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("selection_copy_overrides");
        let migrated: AppConfig = serde_json::from_value(legacy).unwrap();
        assert!(migrated.selection_copy_overrides.is_empty());
    }

    #[test]
    fn independent_remote_profiles_validate_and_round_trip() {
        let mut config = AppConfig::default();
        config.speech.provider = SpeechProvider::OpenAiCompatible {
            base_url: "https://voice.example/v1".into(),
            model: "voice".into(),
            voice: "alloy".into(),
            format: "wav".into(),
        };
        config.ocr.provider = OcrProvider::OpenAiCompatible {
            base_url: "https://vision.example/v1".into(),
            model: "vision".into(),
            api: OcrApi::Responses,
        };
        config.validate().unwrap();
        let encoded = serde_json::to_string(&config).unwrap();
        let decoded: AppConfig = serde_json::from_str(&encoded).unwrap();
        assert_ne!(decoded.speech.provider, SpeechProvider::LocalKokoro);
        assert_ne!(decoded.ocr.provider, OcrProvider::LocalPaddle);
    }

    #[test]
    fn revisions_prevent_silent_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(temp.path().join("config.json"));
        let first = store.save(AppConfig::default(), 1).unwrap();
        assert_eq!(first.revision, 2);
        assert!(store.save(AppConfig::default(), 1).is_err());
    }

    #[test]
    fn concurrent_saves_serialize_the_revision_check() {
        let temp = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(temp.path().join("config.json"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let writers = (0..2)
            .map(|_| {
                let store = store.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.save(AppConfig::default(), 1)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let successes = writers
            .into_iter()
            .map(|writer| writer.join().unwrap().is_ok())
            .filter(|success| *success)
            .count();
        assert_eq!(successes, 1);
        assert_eq!(store.load().unwrap().revision, 2);
    }

    #[test]
    fn config_rejects_unsafe_provider_urls_and_limits() {
        let mut config = AppConfig::default();
        config.speech.provider = SpeechProvider::OpenAiCompatible {
            base_url: "file:///etc/passwd".into(),
            model: "v".into(),
            voice: "x".into(),
            format: "pcm".into(),
        };
        assert!(config.validate().is_err());
        config.speech.provider = SpeechProvider::LocalKokoro;
        config.playback.max_buffer_seconds = 9;
        assert!(config.validate().is_err());
    }

    #[test]
    fn legacy_playback_shortcuts_are_ignored_and_removed_on_save() {
        let mut legacy = serde_json::to_value(AppConfig::default()).unwrap();
        legacy["shortcuts"]["pause_resume"] = serde_json::json!("Ctrl+Alt+P");
        legacy["shortcuts"]["replay_sentence"] = serde_json::json!("Ctrl+Alt+Left");
        legacy["shortcuts"]["next_sentence"] = serde_json::json!("Ctrl+Alt+Right");
        legacy["shortcuts"]["previous_sentence"] = serde_json::json!("Ctrl+Alt+Up");
        legacy["shortcuts"]["stop"] = serde_json::json!("Ctrl+Alt+Escape");
        legacy["shortcuts"]
            .as_object_mut()
            .unwrap()
            .remove("listen_clipboard");
        legacy["shortcuts"]["read_clipboard"] = serde_json::json!("Ctrl+Alt+C");

        let migrated: AppConfig = serde_json::from_value(legacy).unwrap();
        let shortcuts = serde_json::to_value(migrated.shortcuts).unwrap();
        assert_eq!(shortcuts.as_object().unwrap().len(), 5);
        assert!(shortcuts.get("stop").is_none());
        assert_eq!(shortcuts["read_selection"], "Ctrl+Alt+R");
        assert_eq!(shortcuts["read_region"], "Ctrl+Alt+O");
        assert_eq!(shortcuts["read_along"], "None");
        assert_eq!(shortcuts["copy_region"], "None");
        assert_eq!(shortcuts["listen_clipboard"], "Ctrl+Alt+C");
        let custom: ShortcutSettings = serde_json::from_value(serde_json::json!({
            "read_selection": "Meta+R", "read_region": "Meta+O",
            "read_along": "Meta+A", "listen_clipboard": "Meta+C", "copy_region": "Meta+Shift+O"
        }))
        .unwrap();
        assert_eq!(custom.read_selection, "Meta+R");
        assert_eq!(custom.read_region, "Meta+O");
        assert_eq!(custom.read_along, "Meta+A");
        assert_eq!(custom.listen_clipboard, "Meta+C");
        assert_eq!(custom.copy_region, "Meta+Shift+O");
    }
}
