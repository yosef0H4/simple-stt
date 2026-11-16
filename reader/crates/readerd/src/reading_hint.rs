use reader_core::config::{data_dir, AppConfig, SpeechProvider};
use reader_core::routing::{Feature, SelectionMode};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Hint {
    language: String,
    runtime: String,
    model: String,
    device: reader_core::routing::Device,
}

fn valid_code(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8 && value.bytes().all(|b| b.is_ascii_lowercase())
}

fn read_hint(path: &std::path::Path) -> Option<Hint> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > 4096 {
        return None;
    }
    serde_json::from_reader(file).ok()
}

fn resolve_hint<T>(
    saved: &AppConfig,
    hint: Hint,
    resolve: impl FnOnce(&str) -> Option<T>,
) -> Option<T> {
    if saved.speech.selection.mode != SelectionMode::AutoDetect || !valid_code(&hint.language) {
        return None;
    }
    let current = saved.speech.selection.binding(Some(&hint.language));
    if current.runtime.as_deref() != Some(hint.runtime.as_str())
        || current.model.as_deref() != Some(hint.model.as_str())
        || current.device != hint.device
    {
        return None;
    }
    resolve(&hint.language)
}

pub fn prewarm(saved: &AppConfig, language: Option<&str>) -> anyhow::Result<AppConfig> {
    if let Some(config) = read_hint(&data_dir().join("last-tts-route.json")).and_then(|hint| {
        resolve_hint(saved, hint, |lang| {
            saved.routed(Feature::Tts, Some(lang)).ok()
        })
    }) {
        return Ok(config);
    }
    saved.prewarm_config(Feature::Tts, language)
}

pub fn record(config: &AppConfig) -> anyhow::Result<()> {
    if !matches!(
        config.speech.provider,
        SpeechProvider::LocalKokoro | SpeechProvider::LocalSupertonic
    ) {
        return Ok(());
    }
    let language = config
        .speech
        .language
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !valid_code(&language) {
        return Ok(());
    }
    let binding = config.speech.selection.binding(Some(&language));
    let (Some(runtime), Some(model)) = (binding.runtime, binding.model) else {
        return Ok(());
    };
    let hint = Hint {
        language,
        runtime,
        model,
        device: binding.device,
    };
    write_hint(&data_dir(), &hint)
}

fn write_hint(dir: &std::path::Path, hint: &Hint) -> anyhow::Result<()> {
    if read_hint(&dir.join("last-tts-route.json")).as_ref() == Some(hint) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".last-tts-route-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> anyhow::Result<()> {
        let mut file = options.open(&tmp)?;
        serde_json::to_writer(&mut file, hint)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&tmp, dir.join("last-tts-route.json"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_core::routing::{Binding, Device};
    fn example() -> (AppConfig, Hint) {
        let mut config = AppConfig::default();
        config.speech.selection.mode = SelectionMode::AutoDetect;
        config.speech.selection.languages.insert(
            "ar".into(),
            Binding {
                runtime: Some("sherpa-cpu".into()),
                model: Some("supertonic3".into()),
                device: Device::Cpu,
                ..Default::default()
            },
        );
        let hint = Hint {
            language: "ar".into(),
            runtime: "sherpa-cpu".into(),
            model: "supertonic3".into(),
            device: Device::Cpu,
        };
        (config, hint)
    }
    #[test]
    fn voice_options_do_not_invalidate_prewarm_hint() {
        let (mut config, hint) = example();
        let binding = config.speech.selection.languages.get_mut("ar").unwrap();
        binding.voice = Some("sid:3".into());
        binding.steps = Some(8);
        assert_eq!(
            resolve_hint(&config, hint, |language| Some(language.to_owned())),
            Some("ar".into())
        );
    }
    #[test]
    fn rejects_invalid_or_changed_hint_and_missing_assets() {
        let (mut config, hint) = example();
        assert_eq!(
            resolve_hint(&config, hint, |lang| Some(lang.to_owned())).as_deref(),
            Some("ar")
        );
        let (_, hint) = example();
        assert!(resolve_hint::<()>(&config, hint, |_| None).is_none()); // deleted model/runtime
        let (_, hint) = example();
        config
            .speech
            .selection
            .languages
            .get_mut("ar")
            .unwrap()
            .model = Some("other".into());
        assert!(resolve_hint(&config, hint, |_| Some(())).is_none());
        let (_, mut hint) = example();
        hint.language = "../../x".into();
        assert!(resolve_hint(&config, hint, |_| Some(())).is_none());
    }
    #[test]
    fn writes_private_atomic_hint_without_text() {
        let dir = std::env::temp_dir().join(format!("reader-hint-test-{}", uuid::Uuid::new_v4()));
        let (_, hint) = example();
        write_hint(&dir, &hint).unwrap();
        let path = dir.join("last-tts-route.json");
        let recovered = read_hint(&path).unwrap();
        assert_eq!(recovered.language, "ar");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
