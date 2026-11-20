//! Shared engine capabilities and language routing. No runtime loading or desktop access.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Stt,
    Tts,
    Ocr,
}
impl Feature {
    pub fn id(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Tts => "tts",
            Self::Ocr => "ocr",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    #[default]
    SingleModel,
    FollowKeyboard,
    AutoDetect,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    #[default]
    Auto,
    Cpu,
    Gpu,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub base_url: String,
    pub model: String,
    pub voice: String,
    pub format: String,
    pub key_slot: Option<String>,
    pub timeout_ms: u64,
}
impl Default for Connection {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: String::new(),
            voice: String::new(),
            format: "wav".into(),
            key_slot: None,
            timeout_ms: 60_000,
        }
    }
}
impl Connection {
    pub fn validate(&self, feature: Feature) -> Result<()> {
        let operation = if feature == Feature::Stt {
            "audio/transcriptions"
        } else {
            "audio/speech"
        };
        let url = crate::provider_url::operation(&self.base_url, operation)?;
        if feature == Feature::Stt {
            crate::provider_url::validate_stt_transport(&url)?;
        }
        anyhow::ensure!(
            self.model.len() <= 256 && self.voice.len() <= 256,
            "Remote model or voice is too long"
        );
        anyhow::ensure!(
            matches!(self.format.as_str(), "wav" | "pcm"),
            "Remote speech format must be wav or pcm"
        );
        anyhow::ensure!(
            (1000..=300000).contains(&self.timeout_ms),
            "Remote timeout must be between 1000 and 300000 milliseconds"
        );
        if let Some(slot) = &self.key_slot {
            crate::secrets::validate_slot(slot)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RemoteRoute {
    pub enabled: bool,
    pub primary: Connection,
    pub fallback_to_local: bool,
}
impl RemoteRoute {
    pub fn validate(&self, feature: Feature) -> Result<()> {
        if self.enabled {
            self.primary.validate(feature)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteRoute>,
    pub device: Device,
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub voice: Option<String>,
    pub steps: Option<u32>,
    pub speed_percent: Option<u16>,
    pub lowercase: bool,
    pub currency_words: bool,
    pub clean_text: bool,
}
impl Default for Binding {
    fn default() -> Self {
        Self {
            remote: None,
            device: Device::Auto,
            runtime: None,
            model: None,
            voice: None,
            steps: None,
            speed_percent: None,
            lowercase: false,
            currency_words: true,
            clean_text: true,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Selection {
    pub mode: SelectionMode,
    pub single: Binding,
    pub languages: BTreeMap<String, Binding>,
}
impl Selection {
    pub fn binding(&self, language: Option<&str>) -> Binding {
        match self.mode {
            SelectionMode::SingleModel => self.single.clone(),
            SelectionMode::FollowKeyboard | SelectionMode::AutoDetect => language
                .and_then(|l| self.languages.get(&primary(l)))
                .cloned()
                .unwrap_or_default(),
        }
    }
    pub fn validate(&self, feature: Feature) -> Result<()> {
        anyhow::ensure!(
            self.mode != SelectionMode::AutoDetect || feature == Feature::Tts,
            "Text language detection is only available for TTS"
        );
        anyhow::ensure!(self.languages.len() <= 128, "Too many keyboard assignments");
        for (lang, binding) in self
            .languages
            .iter()
            .map(|(l, b)| (l.as_str(), b))
            .chain([("single", &self.single)])
        {
            anyhow::ensure!(
                !binding.lowercase
                    || (feature == Feature::Tts && binding.model.as_deref() == Some("kokoro")),
                "Lowercase text is only supported by Kokoro"
            );
            if let Some(remote) = &binding.remote {
                remote.validate(feature)?;
            }
            if let Some(speed) = binding.speed_percent {
                anyhow::ensure!(
                    feature == Feature::Tts && (50..=200).contains(&speed),
                    "Speech speed must be between 50 and 200 percent and requires TTS"
                );
            }
            if binding.voice.is_some() || binding.steps.is_some() {
                anyhow::ensure!(feature == Feature::Tts, "Voice options require a TTS model");
                match binding.model.as_deref() {
                    Some("kokoro") => {
                        anyhow::ensure!(
                            binding.steps.is_none(),
                            "Steps are only supported by Supertonic"
                        );
                        if let Some(voice) = binding.voice.as_deref() {
                            anyhow::ensure!(
                                [
                                    "af_heart",
                                    "af_bella",
                                    "af_nicole",
                                    "af_sarah",
                                    "am_adam",
                                    "am_michael",
                                    "bf_emma"
                                ]
                                .contains(&voice),
                                "Unsupported Kokoro voice"
                            );
                        }
                    }
                    Some("supertonic3") => {
                        if let Some(voice) = binding.voice.as_deref() {
                            anyhow::ensure!(
                                (0..10).any(|id| voice == format!("sid:{id}")),
                                "Unsupported Supertonic voice"
                            );
                        }
                        anyhow::ensure!(
                            binding.steps.is_none_or(|steps| (5..=12).contains(&steps)),
                            "Supertonic steps must be between 5 and 12"
                        );
                    }
                    _ => anyhow::bail!("Voice options require a supported TTS model"),
                }
            }
            anyhow::ensure!(
                !lang.is_empty()
                    && lang.len() <= 32
                    && lang
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Invalid keyboard language"
            );
            if let Some(id) = &binding.runtime {
                let runtime = registry()
                    .runtimes
                    .into_iter()
                    .find(|r| r.id == *id && r.feature == feature)
                    .context("Unsupported runtime")?;
                anyhow::ensure!(
                    binding.device != Device::Gpu || runtime.backend != "cpu",
                    "Selected runtime supports CPU only"
                );
                if let Some(model) = &binding.model {
                    let model = registry()
                        .models
                        .into_iter()
                        .find(|m| m.id == *model && m.feature == feature)
                        .context("Unsupported model")?;
                    anyhow::ensure!(
                        runtime.engine == model.engine,
                        "Runtime and model are incompatible"
                    );
                }
            }
            if let Some(id) = &binding.model {
                anyhow::ensure!(
                    registry()
                        .models
                        .iter()
                        .any(|m| m.id == *id && m.feature == feature),
                    "Unsupported model"
                );
            }
        }
        Ok(())
    }
}
pub fn primary(language: &str) -> String {
    language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub id: String,
    pub feature: Feature,
    pub engine: String,
    pub backend: String,
    pub label: String,
    pub asset: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platform_assets: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub feature: Feature,
    pub engine: String,
    pub label: String,
    pub languages: Vec<String>,
    pub asset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paddle: Option<crate::paddle::PaddleProfile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub runtimes: Vec<Runtime>,
    pub models: Vec<Model>,
}
pub fn registry() -> Registry {
    let mut registry: Registry =
        serde_json::from_str(include_str!("../../../../assets/catalog/modules.json"))
            .expect("validated module registry");
    for runtime in &mut registry.runtimes {
        if let Some(asset) = runtime.platform_assets.get(std::env::consts::OS) {
            runtime.asset = Some(asset.clone());
        }
        #[cfg(windows)]
        if runtime.id == "parakeet-cuda" {
            runtime.label = "Parakeet · CUDA 12.8 (Blackwell)".into();
        }
    }
    registry
}

#[derive(Deserialize)]
struct AssetReceipt {
    id: String,
    sha256: String,
    required: Vec<String>,
}

pub fn asset_installed(id: &str, assets: &std::path::Path) -> bool {
    let catalog: Vec<AssetReceipt> =
        serde_json::from_str(include_str!("../../../../assets/catalog/downloads.json"))
            .expect("validated asset catalog");
    let Some(asset) = catalog.iter().find(|asset| asset.id == id) else {
        return false;
    };
    let directory = assets.join(id);
    std::fs::read_to_string(directory.join(".verified"))
        .is_ok_and(|receipt| receipt == asset.sha256)
        && asset
            .required
            .iter()
            .all(|file| directory.join(file).exists())
}

pub fn runtime_installed(runtime: &Runtime, assets: &std::path::Path) -> bool {
    match &runtime.asset {
        Some(id) => asset_installed(id, assets),
        None => false,
    }
}
pub fn resolve(
    selection: &Selection,
    feature: Feature,
    language: Option<&str>,
    assets: &std::path::Path,
) -> Result<(Runtime, Model)> {
    selection.validate(feature)?;
    let binding = selection.binding(language);
    let registry = registry();
    let runtime = binding.runtime.as_ref().and_then(|id| {
        registry
            .runtimes
            .iter()
            .find(|r| r.id == *id && r.feature == feature)
    });
    let model = binding.model.as_ref().and_then(|id| {
        registry
            .models
            .iter()
            .find(|r| r.id == *id && r.feature == feature)
    });
    let runtime_ready = runtime.is_some_and(|r| runtime_installed(r, assets));
    let model_ready = model.is_some_and(|m| asset_installed(&m.asset, assets));
    anyhow::ensure!(
        runtime_ready && model_ready,
        "{}: runtime {} · model {}. Open Settings.",
        feature.id().to_uppercase(),
        if runtime_ready { "✓" } else { "missing" },
        if model_ready { "✓" } else { "missing" }
    );
    let runtime = runtime.unwrap().clone();
    let model = model.unwrap().clone();
    if selection.mode != SelectionMode::SingleModel {
        let code = primary(language.context("Keyboard language unavailable")?);
        anyhow::ensure!(
            model.languages.is_empty() || model.languages.contains(&code),
            "{} model does not support keyboard language {code}",
            feature.id().to_uppercase()
        );
    }
    Ok((runtime, model))
}

fn ocr_device(device: Device) -> crate::DevicePreference {
    match device {
        Device::Gpu => crate::DevicePreference::Cuda,
        Device::Auto | Device::Cpu => crate::DevicePreference::Cpu,
    }
}

impl crate::AppConfig {
    /// Speculate using a configured language only; never create a general route.
    pub fn prewarm_config(&self, feature: Feature, language: Option<&str>) -> Result<Self> {
        if feature != Feature::Tts || self.speech.selection.mode != SelectionMode::AutoDetect {
            return self.routed(feature, language);
        }
        let complete = |binding: &&Binding| binding.runtime.is_some() && binding.model.is_some();
        let (language, _) = self
            .speech
            .selection
            .languages
            .get_key_value("en")
            .filter(|(_, binding)| complete(binding))
            .or_else(|| {
                self.speech
                    .selection
                    .languages
                    .iter()
                    .find(|(_, binding)| complete(binding))
            })
            .context("TTS: choose a runtime and model for a language in Settings")?;
        self.routed(feature, Some(language))
    }
    pub fn routed_text(&self, text: &str, keyboard: Option<&str>) -> Result<Self> {
        let language = if self.speech.selection.mode == SelectionMode::AutoDetect {
            crate::language::resolve(text)
        } else {
            keyboard.unwrap_or(&self.speech.language).to_owned()
        };
        self.routed(Feature::Tts, Some(&language))
    }
    pub fn routed(&self, feature: Feature, language: Option<&str>) -> Result<Self> {
        let mut config = self.clone();
        let defaults = self.speech.route_defaults.clone().unwrap_or_else(|| {
            crate::config::SpeechRouteDefaults {
                provider: self.speech.provider.clone(),
                key_slot: self.speech.key_slot.clone(),
                use_feature_key: self.speech.use_feature_key,
                fallback_to_local: self.speech.fallback_to_local,
            }
        });
        config.speech.provider = defaults.provider.clone();
        config.speech.key_slot = defaults.key_slot.clone();
        config.speech.use_feature_key = defaults.use_feature_key;
        config.speech.fallback_to_local = defaults.fallback_to_local;
        config.speech.route_defaults = Some(defaults);
        let binding = self.speech.selection.binding(language);
        let mut explicit_remote = false;
        if feature == Feature::Tts {
            if let Some(remote) = &binding.remote {
                if remote.enabled {
                    explicit_remote = true;
                    remote.validate(feature)?;
                    config.speech.provider = crate::SpeechProvider::OpenAiCompatible {
                        base_url: remote.primary.base_url.clone(),
                        model: remote.primary.model.clone(),
                        voice: remote.primary.voice.clone(),
                        format: remote.primary.format.clone(),
                    };
                    config.speech.key_slot = remote.primary.key_slot.clone();
                    config.speech.use_feature_key = false;
                    config.speech.fallback_to_local = remote.fallback_to_local;
                } else {
                    config.speech.provider = crate::SpeechProvider::LocalKokoro;
                }
            }
            if matches!(
                config.speech.provider,
                crate::SpeechProvider::OpenAiCompatible { .. }
            ) {
                if config.speech.fallback_to_local {
                    let mut local = self.clone();
                    local.speech.route_defaults = None;
                    local.speech.provider = crate::SpeechProvider::LocalKokoro;
                    local.speech.selection.mode = SelectionMode::SingleModel;
                    local.speech.selection.single = binding.clone();
                    local.speech.selection.single.remote = None;
                    match local.routed(feature, language) {
                        Ok(local) => {
                            config.speech.model_dir = local.speech.model_dir;
                            config.speech.runtime_dir = local.speech.runtime_dir;
                            config.speech.device = local.speech.device;
                            config.speech.voice = local.speech.voice;
                        }
                        Err(_) if explicit_remote => {
                            config.speech.model_dir =
                                crate::config::data_dir().join("unconfigured-local-fallback");
                            config.speech.runtime_dir = config.speech.model_dir.clone();
                        }
                        Err(_) => {}
                    }
                }
                config.speech.selection.single = binding;
                config.speech.language = language
                    .map(primary)
                    .unwrap_or_else(|| self.speech.language.clone());
                return Ok(config);
            }
        }
        let assets = crate::config::data_dir()
            .parent()
            .context("Reader data has no parent")?
            .join("assets");
        match feature {
            Feature::Tts
                if matches!(
                    config.speech.provider,
                    crate::SpeechProvider::LocalKokoro | crate::SpeechProvider::LocalSupertonic
                ) =>
            {
                let (runtime, model) = resolve(&self.speech.selection, feature, language, &assets)?;
                config.speech.key_slot = None;
                config.speech.runtime_dir =
                    assets.join(runtime.asset.context("Speech runtime has no asset")?);
                config.speech.model_dir = assets.join(model.asset);
                let binding = self.speech.selection.binding(language);
                config.speech.selection.single = binding.clone();
                config.speech.speed = f32::from(binding.speed_percent.unwrap_or(100)) / 100.0;
                config.speech.device = if binding.device == Device::Cpu {
                    crate::DevicePreference::Cpu
                } else if runtime.backend == "cuda" {
                    if binding.device == Device::Gpu {
                        crate::DevicePreference::Cuda
                    } else {
                        crate::DevicePreference::Auto
                    }
                } else {
                    crate::DevicePreference::Cpu
                };
                config.speech.language = language
                    .map(primary)
                    .unwrap_or_else(|| self.speech.language.clone());
                if model.id == "supertonic3" {
                    config.speech.provider = crate::SpeechProvider::LocalSupertonic;
                    config.speech.voice = binding.voice.clone().unwrap_or_else(|| "sid:0".into());
                } else {
                    config.speech.provider = crate::SpeechProvider::LocalKokoro;
                    config.speech.voice =
                        binding.voice.clone().unwrap_or_else(|| "af_heart".into());
                }
                anyhow::ensure!(
                    model.languages.is_empty()
                        || model.languages.contains(&primary(&config.speech.language)),
                    "Selected TTS model does not support {}",
                    config.speech.language
                );
            }
            Feature::Ocr => {
                let local = matches!(self.ocr.provider, crate::OcrProvider::LocalPaddle);
                if !local && !self.ocr.fallback_to_local {
                    return Ok(config);
                }
                let (runtime, model) =
                    match resolve(&self.ocr.selection, feature, language, &assets) {
                        Ok(assignment) => assignment,
                        Err(error) if local => return Err(error),
                        Err(_) => return Ok(config),
                    };
                config.ocr.model_dir = assets.join(&model.asset);
                config.ocr.runtime_dir =
                    assets.join(runtime.asset.context("OCR runtime has no asset")?);
                config.ocr.device = ocr_device(self.ocr.selection.binding(language).device);
            }
            _ => {}
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_secondary_endpoints_are_ignored_without_changing_local_fallback() {
        let mut json = serde_json::to_value(crate::AppConfig::default()).unwrap();
        json["speech"]["remote_fallback"] = serde_json::json!({"base_url":"invalid"});
        json["speech"]["selection"]["languages"]["ar"] = serde_json::json!({
            "remote": {
                "enabled": true,
                "primary": {"base_url":"https://primary.example/v1", "model":"speech"},
                "fallback": {"base_url":"invalid"},
                "fallback_to_local": true
            }
        });
        let config: crate::AppConfig = serde_json::from_value(json).unwrap();
        config.validate().unwrap();
        let canonical = serde_json::to_value(config).unwrap();
        assert!(canonical["speech"].get("remote_fallback").is_none());
        let route = &canonical["speech"]["selection"]["languages"]["ar"]["remote"];
        assert!(route.get("fallback").is_none());
        assert_eq!(route["primary"]["base_url"], "https://primary.example/v1");
        assert_eq!(route["fallback_to_local"], true);
    }
    #[test]
    fn local_language_routing_selects_independent_remote_models_voices_and_keys() {
        let mut config = crate::AppConfig::default();
        config.speech.selection.mode = SelectionMode::AutoDetect;
        for language in ["en", "ar"] {
            config.speech.selection.languages.insert(
                language.into(),
                Binding {
                    remote: Some(RemoteRoute {
                        enabled: true,
                        primary: Connection {
                            base_url: format!("https://{language}.example/v1"),
                            model: format!("{language}-tts"),
                            voice: format!("{language}-voice"),
                            key_slot: Some(format!("tts.{language}.primary")),
                            ..Default::default()
                        },
                        fallback_to_local: false,
                    }),
                    ..Default::default()
                },
            );
        }
        config.validate().unwrap();
        let ar = config.routed(Feature::Tts, Some("ar")).unwrap();
        let en = ar.routed(Feature::Tts, Some("en")).unwrap();
        assert_eq!(ar.speech.key_slot.as_deref(), Some("tts.ar.primary"));
        assert_eq!(en.speech.key_slot.as_deref(), Some("tts.en.primary"));
        assert!(!en.speech.use_feature_key);
        match en.speech.provider {
            crate::SpeechProvider::OpenAiCompatible {
                model,
                voice,
                base_url,
                ..
            } => {
                assert_eq!(model, "en-tts");
                assert_eq!(voice, "en-voice");
                assert_eq!(base_url, "https://en.example/v1");
            }
            _ => panic!("expected remote route"),
        }
        assert!(
            ar.routed(Feature::Tts, Some("fr")).is_err(),
            "an unassigned language must not inherit the preceding remote server"
        );
        assert_eq!(config.speech.selection.mode, SelectionMode::AutoDetect);
    }

    #[test]
    fn automatic_languages_never_inherit_a_general_assignment() {
        let selection: Selection = serde_json::from_value(serde_json::json!({
            "mode":"auto_detect",
            "single":{"runtime":"sherpa-cpu","model":"kokoro"},
            "fallback":{"runtime":"sherpa-cpu","model":"supertonic3"},
            "uncertain_language":"ar",
            "languages":{"en":{"runtime":"sherpa-cpu","model":"kokoro","speed_percent":130}}
        }))
        .unwrap();
        assert_eq!(selection.binding(Some("ar")), Binding::default());
        assert_eq!(selection.binding(None), Binding::default());
        assert_eq!(
            selection.binding(Some("en-US")).model.as_deref(),
            Some("kokoro")
        );
        let saved = serde_json::to_value(selection).unwrap();
        assert!(saved.get("fallback").is_none());
        assert!(saved.get("uncertain_language").is_none());
    }
    #[test]
    fn ocr_auto_prefers_cpu_and_gpu_remains_explicit() {
        assert_eq!(ocr_device(Device::Auto), crate::DevicePreference::Cpu);
        assert_eq!(ocr_device(Device::Cpu), crate::DevicePreference::Cpu);
        assert_eq!(ocr_device(Device::Gpu), crate::DevicePreference::Cuda);
    }
    #[test]
    fn language_routes_have_no_implicit_fallback() {
        let mut selection = Selection {
            mode: SelectionMode::FollowKeyboard,
            ..Default::default()
        };
        selection.languages.insert(
            "en".into(),
            Binding {
                device: Device::Auto,
                runtime: Some("sherpa-cpu".into()),
                model: Some("kokoro".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            selection.binding(Some("en-US")).model.as_deref(),
            Some("kokoro")
        );
        assert_eq!(selection.binding(Some("ar")), Binding::default());
        selection.validate(Feature::Tts).unwrap();
        assert!(selection.validate(Feature::Ocr).is_err());
    }
    #[test]
    fn language_speed_is_independent_and_validated() {
        let mut selection = Selection {
            mode: SelectionMode::AutoDetect,
            ..Default::default()
        };
        selection.languages.insert(
            "ar".into(),
            Binding {
                speed_percent: Some(90),
                ..Default::default()
            },
        );
        selection.languages.insert(
            "en".into(),
            Binding {
                speed_percent: Some(130),
                ..Default::default()
            },
        );
        assert_eq!(selection.binding(Some("en-US")).speed_percent, Some(130));
        assert_eq!(selection.binding(Some("ar")).speed_percent, Some(90));
        let value = serde_json::to_value(&selection).unwrap();
        assert_eq!(
            serde_json::from_value::<Selection>(value).unwrap(),
            selection
        );
        selection.validate(Feature::Tts).unwrap();
        selection.languages.get_mut("ar").unwrap().speed_percent = Some(201);
        assert!(selection.validate(Feature::Tts).is_err());
        selection.languages.get_mut("ar").unwrap().speed_percent = Some(49);
        assert!(selection.validate(Feature::Tts).is_err());
        assert_eq!(
            serde_json::from_str::<Binding>("{}").unwrap().speed_percent,
            None
        );
        let ocr = Selection {
            single: Binding {
                speed_percent: Some(100),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(ocr.validate(Feature::Ocr).is_err());
    }
    #[test]
    fn incompatible_pair_is_rejected() {
        let s = Selection {
            single: Binding {
                device: Device::Auto,
                runtime: Some("sherpa-cpu".into()),
                model: Some("ocr-eng".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(s.validate(Feature::Tts).is_err());
    }

    #[test]
    fn speech_options_round_trip_and_are_validated_per_model() {
        let old: Binding = serde_json::from_str(r#"{"model":"kokoro"}"#).unwrap();
        assert_eq!(old.voice, None);
        assert_eq!(old.steps, None);
        assert!(!old.lowercase);
        assert!(old.currency_words);
        assert!(old.clean_text);
        let serialized = serde_json::to_value(&old).unwrap();
        assert_eq!(serialized["clean_text"], true);
        assert_eq!(serde_json::from_value::<Binding>(serialized).unwrap(), old);
        let mut selection = Selection {
            single: Binding {
                model: Some("supertonic3".into()),
                voice: Some("sid:9".into()),
                steps: Some(12),
                ..Default::default()
            },
            ..Default::default()
        };
        let value = serde_json::to_value(&selection).unwrap();
        assert_eq!(value["single"]["voice"], "sid:9");
        assert_eq!(value["single"]["steps"], 12);
        assert_eq!(
            serde_json::from_value::<Selection>(value).unwrap(),
            selection
        );
        assert!(selection.validate(Feature::Tts).is_ok());
        selection.single.steps = Some(13);
        assert!(selection.validate(Feature::Tts).is_err());
        selection.single.steps = Some(5);
        selection.single.voice = Some("af_heart".into());
        assert!(selection.validate(Feature::Tts).is_err());
        selection.single.model = Some("kokoro".into());
        assert!(selection.validate(Feature::Tts).is_err());
        selection.single.steps = None;
        assert!(selection.validate(Feature::Tts).is_ok());
        selection.single.lowercase = true;
        assert!(selection.validate(Feature::Tts).is_ok());
        selection.single.model = Some("supertonic3".into());
        selection.single.voice = None;
        assert!(selection.validate(Feature::Tts).is_err());
        assert!(selection.validate(Feature::Ocr).is_err());
    }
}
