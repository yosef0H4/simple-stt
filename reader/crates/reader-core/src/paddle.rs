//! Declarative detector/recognizer/dictionary bundles for the OCR worker.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectionPreset {
    #[default]
    Default,
    V6Tiny,
    V6,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PaddleProfile {
    pub detector: String,
    pub recognizer: String,
    pub dictionary: String,
    #[serde(default)]
    pub reverse_arabic: bool,
    #[serde(default)]
    pub detection_preset: DetectionPreset,
}

impl PaddleProfile {
    pub fn files(&self) -> [&str; 3] {
        [&self.detector, &self.recognizer, &self.dictionary]
    }

    pub fn validate(&self) -> Result<()> {
        for file in self.files() {
            anyhow::ensure!(
                !file.is_empty() && !file.contains(['/', '\\']) && file != "." && file != "..",
                "Paddle model files must be plain filenames"
            );
        }
        Ok(())
    }

    fn present(&self, directory: &Path) -> bool {
        self.files()
            .iter()
            .all(|name| directory.join(name).is_file())
    }

    /// Asset folders select their exact profile. A custom folder is accepted only
    /// when its files identify one complete supported bundle without ambiguity.
    pub fn for_directory(directory: &Path) -> Result<Self> {
        let registry = crate::routing::registry();
        let profiles: Vec<_> = registry
            .models
            .iter()
            .filter_map(|model| model.paddle.as_ref().map(|profile| (model, profile)))
            .collect();
        if let Some((_, profile)) = profiles.iter().find(|(model, _)| {
            directory.file_name().and_then(|name| name.to_str()) == Some(&model.asset)
        }) {
            profile.validate()?;
            anyhow::ensure!(
                profile.present(directory),
                "Paddle model bundle is incomplete"
            );
            return Ok((*profile).clone());
        }
        let mut matching = profiles
            .iter()
            .filter(|(_, profile)| profile.present(directory));
        let (_, profile) = matching
            .next()
            .context("No complete supported Paddle model found")?;
        if matching.next().is_some() {
            bail!("Multiple Paddle models found; choose a specific model folder")
        }
        profile.validate()?;
        Ok((*profile).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{registry, Feature};

    #[test]
    fn every_ocr_bundle_has_a_matching_download_and_dictionary() {
        let downloads: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../../assets/catalog/downloads.json"))
                .unwrap();
        let models = registry().models;
        for model in models.iter().filter(|model| model.feature == Feature::Ocr) {
            let profile = model.paddle.as_ref().expect("OCR bundle profile");
            profile.validate().unwrap();
            let asset = downloads
                .iter()
                .find(|asset| asset["id"] == model.asset)
                .unwrap();
            for file in profile.files() {
                assert!(
                    asset["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|value| value == file),
                    "{}/{file}",
                    model.id
                );
            }
        }
        assert!(models.iter().filter(|model| model.paddle.is_some()).count() >= 34);
    }

    #[test]
    fn custom_bundle_resolves_by_its_files_and_rejects_ambiguous_folders() {
        let folder = tempfile::tempdir().unwrap();
        let models = registry().models;
        for id in ["paddle-ar-en", "paddle-en-pp-ocrv5-mobile-rec"] {
            let profile = models
                .iter()
                .find(|model| model.id == id)
                .unwrap()
                .paddle
                .as_ref()
                .unwrap();
            for file in profile.files() {
                std::fs::write(folder.path().join(file), b"fixture").unwrap();
            }
            if id == "paddle-ar-en" {
                assert_eq!(
                    PaddleProfile::for_directory(folder.path()).unwrap(),
                    *profile
                );
            }
        }
        assert!(PaddleProfile::for_directory(folder.path())
            .unwrap_err()
            .to_string()
            .contains("Multiple"));
    }

    #[test]
    fn only_legacy_arabic_dictionaries_reverse_recognized_text() {
        for model in registry().models {
            if let Some(profile) = model.paddle {
                assert_eq!(
                    profile.reverse_arabic,
                    profile.dictionary.contains("arabic")
                );
            }
        }
    }
}
