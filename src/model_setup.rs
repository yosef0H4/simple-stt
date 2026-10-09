use crate::config::{replace_file_atomic, unique_atomic_temp_path, AppConfig, ModelSelectionMode};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ARABIC_MODEL: &str = "lemura-arabic-asr-lite-q8_0.gguf";
const STATE_FILE: &str = "recommended-model-setup.json";

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ModelSetupChoice {
    pub english: bool,
    pub arabic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecommendedModels {
    pub english_file: String,
    pub arabic_file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeOutcome {
    NoPendingChoice,
    WaitingForDownloads,
    Applied,
    PreservedUserSelection,
    AlreadyComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SpeechSelection {
    selection_mode: ModelSelectionMode,
    single_model_filename: Option<String>,
    language_models: BTreeMap<String, Option<String>>,
}

impl From<&AppConfig> for SpeechSelection {
    fn from(config: &AppConfig) -> Self {
        Self {
            selection_mode: config.speech.selection_mode,
            single_model_filename: config.speech.single_model_filename.clone(),
            language_models: config.speech.language_models.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SetupRecord {
    Pending {
        english_file: Option<String>,
        arabic_file: Option<String>,
        baseline: SpeechSelection,
    },
    Done,
}

fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILE)
}

fn recommended_models(config: &AppConfig) -> Result<RecommendedModels> {
    let models = crate::models::catalog_for_config(config);
    let english_file = models
        .iter()
        .filter(|model| {
            model.recommended
                && model
                    .languages
                    .iter()
                    .any(|language| language == "English (en)")
        })
        .min_by(|a, b| a.size_mb.cmp(&b.size_mb).then_with(|| a.file.cmp(&b.file)))
        .map(|model| model.file.clone())
        .unwrap_or_else(|| {
            crate::models::recommended_model_for_device(&config.speech.inference_device).to_owned()
        });
    anyhow::ensure!(
        models.iter().any(|model| model.file == english_file),
        "the recommended English model is not in the approved catalog"
    );
    anyhow::ensure!(
        models.iter().any(|model| model.file == ARABIC_MODEL),
        "the recommended Arabic model is not in the approved catalog"
    );
    Ok(RecommendedModels {
        english_file,
        arabic_file: ARABIC_MODEL.to_owned(),
    })
}

pub fn recommended_files(config: &AppConfig) -> Result<RecommendedModels> {
    recommended_models(config)
}

pub fn accept_choice(data_dir: &Path, config: &AppConfig, choice: ModelSetupChoice) -> Result<()> {
    let recommended = recommended_models(config)?;
    let record = SetupRecord::Pending {
        english_file: choice.english.then_some(recommended.english_file),
        arabic_file: choice.arabic.then_some(recommended.arabic_file),
        baseline: SpeechSelection::from(config),
    };
    write_record(data_dir, &record)
}

pub fn prompt_state(data_dir: &Path, models_missing: bool) -> Result<(bool, bool)> {
    let Some(record) = read_record(data_dir)? else {
        return Ok((models_missing, false));
    };
    Ok((false, matches!(record, SetupRecord::Pending { .. })))
}

pub fn finalize_pending(
    data_dir: &Path,
    config_path: &Path,
    config: &mut AppConfig,
) -> Result<FinalizeOutcome> {
    let Some(record) = read_record(data_dir)? else {
        return Ok(FinalizeOutcome::NoPendingChoice);
    };
    let SetupRecord::Pending {
        english_file,
        arabic_file,
        baseline,
    } = record
    else {
        return Ok(FinalizeOutcome::AlreadyComplete);
    };

    if SpeechSelection::from(&*config) != baseline {
        write_record(data_dir, &SetupRecord::Done)?;
        return Ok(FinalizeOutcome::PreservedUserSelection);
    }

    let selected = [english_file.as_deref(), arabic_file.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let catalog = crate::models::catalog_for_config(config);
    for filename in &selected {
        anyhow::ensure!(
            catalog.iter().any(|model| model.file == *filename),
            "pending model setup references a model outside the approved catalog"
        );
    }
    if selected
        .iter()
        .any(|filename| !config.model_dir_path().join(filename).is_file())
    {
        return Ok(FinalizeOutcome::WaitingForDownloads);
    }

    match (english_file, arabic_file) {
        (Some(english), Some(arabic)) => {
            config.speech.selection_mode = ModelSelectionMode::FollowKeyboard;
            config
                .speech
                .language_models
                .insert("en".into(), Some(english));
            config
                .speech
                .language_models
                .insert("ar".into(), Some(arabic));
        }
        (Some(filename), None) | (None, Some(filename)) => {
            config.speech.selection_mode = ModelSelectionMode::SingleModel;
            config.speech.single_model_filename = Some(filename);
        }
        (None, None) => {
            config.speech.selection_mode = ModelSelectionMode::SingleModel;
            config.speech.single_model_filename = None;
        }
    }
    config.save_to(config_path)?;
    write_record(data_dir, &SetupRecord::Done)?;
    Ok(FinalizeOutcome::Applied)
}

fn read_record(data_dir: &Path) -> Result<Option<SetupRecord>> {
    let path = state_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("parsing model setup state {}", path.display()))
        .map(Some)
}

fn write_record(data_dir: &Path, record: &SetupRecord) -> Result<()> {
    fs::create_dir_all(data_dir).with_context(|| format!("creating {}", data_dir.display()))?;
    let path = state_path(data_dir);
    let temp = unique_atomic_temp_path(&path);
    let mut bytes = serde_json::to_vec_pretty(record)?;
    bytes.push(b'\n');
    fs::write(&temp, bytes)?;
    replace_file_atomic(&temp, &path)
        .with_context(|| format!("writing model setup state {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, AppConfig) {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let data_dir = dir.path().join("data");
        let model_dir = dir.path().join("models");
        let mut config = AppConfig::default();
        config.speech.model_dir = model_dir.to_string_lossy().into_owned();
        config.save_to(&config_path).unwrap();
        (dir, config_path, data_dir, config)
    }

    fn install(config: &AppConfig, files: &[&str]) {
        fs::create_dir_all(config.model_dir_path()).unwrap();
        for file in files {
            fs::write(config.model_dir_path().join(file), b"test model").unwrap();
        }
    }

    fn apply(choice: ModelSetupChoice, install_files: bool) -> (FinalizeOutcome, AppConfig) {
        let (_dir, config_path, data_dir, mut config) = setup();
        let recommended = recommended_files(&config).unwrap();
        accept_choice(&data_dir, &config, choice).unwrap();
        if install_files {
            let files = [
                choice.english.then_some(recommended.english_file.as_str()),
                choice.arabic.then_some(recommended.arabic_file.as_str()),
            ];
            install(&config, &files.into_iter().flatten().collect::<Vec<_>>());
        }
        let outcome = finalize_pending(&data_dir, &config_path, &mut config).unwrap();
        (outcome, config)
    }

    #[test]
    fn both_models_apply_keyboard_language_assignments_only_when_installed() {
        let (_dir, config_path, data_dir, mut config) = setup();
        let recommended = recommended_files(&config).unwrap();
        accept_choice(
            &data_dir,
            &config,
            ModelSetupChoice {
                english: true,
                arabic: true,
            },
        )
        .unwrap();
        config.audio.gain = 1.25;
        config.general.ui_language = crate::config::UiLanguage::Ar;
        config.save_to(&config_path).unwrap();
        assert_eq!(
            finalize_pending(&data_dir, &config_path, &mut config).unwrap(),
            FinalizeOutcome::WaitingForDownloads
        );
        assert_eq!(
            config.speech.selection_mode,
            ModelSelectionMode::SingleModel
        );
        install(
            &config,
            &[&recommended.english_file, &recommended.arabic_file],
        );
        let mut reopened = AppConfig::load_from(&config_path).unwrap();
        let outcome = finalize_pending(&data_dir, &config_path, &mut reopened).unwrap();
        assert_eq!(outcome, FinalizeOutcome::Applied);
        assert_eq!(
            reopened.speech.selection_mode,
            ModelSelectionMode::FollowKeyboard
        );
        assert_eq!(
            reopened.speech.language_models.get("en"),
            Some(&Some(recommended.english_file))
        );
        assert_eq!(
            reopened.speech.language_models.get("ar"),
            Some(&Some(recommended.arabic_file))
        );
        assert_eq!(reopened.audio.gain, 1.25);
        assert_eq!(reopened.general.ui_language, crate::config::UiLanguage::Ar);
    }

    #[test]
    fn already_installed_selection_applies_on_next_state_read() {
        let (_dir, config_path, data_dir, mut config) = setup();
        let recommended = recommended_files(&config).unwrap();
        install(&config, &[&recommended.english_file]);
        accept_choice(
            &data_dir,
            &config,
            ModelSetupChoice {
                english: true,
                arabic: false,
            },
        )
        .unwrap();
        assert_eq!(
            finalize_pending(&data_dir, &config_path, &mut config).unwrap(),
            FinalizeOutcome::Applied
        );
        assert_eq!(
            config.speech.single_model_filename.as_deref(),
            Some(recommended.english_file.as_str())
        );
    }

    #[test]
    fn either_single_model_and_skip_apply_the_requested_mode() {
        for choice in [
            ModelSetupChoice {
                english: true,
                arabic: false,
            },
            ModelSetupChoice {
                english: false,
                arabic: true,
            },
        ] {
            let (outcome, config) = apply(choice, true);
            assert_eq!(outcome, FinalizeOutcome::Applied);
            assert_eq!(
                config.speech.selection_mode,
                ModelSelectionMode::SingleModel
            );
            let selected = config.speech.single_model_filename.as_deref().unwrap();
            let recommended = recommended_files(&config).unwrap();
            assert_eq!(
                selected,
                if choice.english {
                    recommended.english_file
                } else {
                    recommended.arabic_file
                }
            );
        }
        let (outcome, config) = apply(
            ModelSetupChoice {
                english: false,
                arabic: false,
            },
            true,
        );
        assert_eq!(outcome, FinalizeOutcome::Applied);
        assert_eq!(
            config.speech.selection_mode,
            ModelSelectionMode::SingleModel
        );
        assert_eq!(config.speech.single_model_filename, None);
    }

    #[test]
    fn pending_choice_survives_reopen_and_preserves_manual_model_edits() {
        let (_dir, config_path, data_dir, mut config) = setup();
        accept_choice(
            &data_dir,
            &config,
            ModelSetupChoice {
                english: true,
                arabic: true,
            },
        )
        .unwrap();
        let (show, pending) = prompt_state(&data_dir, true).unwrap();
        assert!(!show);
        assert!(pending);
        config.speech.single_model_filename = Some("manual-choice.gguf".into());
        config.save_to(&config_path).unwrap();
        let mut reopened = AppConfig::load_from(&config_path).unwrap();
        let outcome = finalize_pending(&data_dir, &config_path, &mut reopened).unwrap();
        assert_eq!(outcome, FinalizeOutcome::PreservedUserSelection);
        assert_eq!(
            reopened.speech.selection_mode,
            ModelSelectionMode::SingleModel
        );
        assert_eq!(
            reopened.speech.single_model_filename.as_deref(),
            Some("manual-choice.gguf")
        );
        assert!(!prompt_state(&data_dir, true).unwrap().0);
    }
}
