use crate::config::{replace_file_atomic, validate_model_filename, AppConfig, InferenceDevice};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const BASE_URL: &str = "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main";
pub const CATALOG_URL: &str = "https://huggingface.co/api/models/mudler/parakeet-cpp-gguf/tree/main?recursive=false&expand=false";

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub family: String,
    pub quant: String,
    pub file: String,
    pub size_mb: u32,
    pub recommended: bool,
    pub installed: bool,
}
const FAMILIES: &[(&str, &[(&str, u32)])] = &[
    (
        "tdt_ctc-110m",
        &[
            ("f16", 268),
            ("q8_0", 178),
            ("q6_k", 156),
            ("q5_k", 143),
            ("q4_k", 131),
        ],
    ),
    (
        "realtime_eou_120m-v1",
        &[
            ("f16", 267),
            ("q8_0", 176),
            ("q6_k", 154),
            ("q5_k", 141),
            ("q4_k", 129),
        ],
    ),
    (
        "ctc-0.6b",
        &[
            ("f16", 1374),
            ("q8_0", 876),
            ("q6_k", 747),
            ("q5_k", 677),
            ("q4_k", 610),
        ],
    ),
    (
        "rnnt-0.6b",
        &[
            ("f16", 1403),
            ("q8_0", 904),
            ("q6_k", 777),
            ("q5_k", 706),
            ("q4_k", 640),
        ],
    ),
    (
        "tdt-0.6b-v2",
        &[
            ("f16", 1405),
            ("q8_0", 904),
            ("q6_k", 776),
            ("q5_k", 705),
            ("q4_k", 639),
        ],
    ),
    (
        "tdt-0.6b-v3",
        &[
            ("f16", 1441),
            ("q8_0", 941),
            ("q6_k", 813),
            ("q5_k", 742),
            ("q4_k", 676),
        ],
    ),
    (
        "ctc-1.1b",
        &[
            ("f16", 2396),
            ("q8_0", 1527),
            ("q6_k", 1302),
            ("q5_k", 1179),
            ("q4_k", 1063),
        ],
    ),
    (
        "rnnt-1.1b",
        &[
            ("f16", 2426),
            ("q8_0", 1555),
            ("q6_k", 1332),
            ("q5_k", 1208),
            ("q4_k", 1092),
        ],
    ),
    (
        "tdt-1.1b",
        &[
            ("f16", 2426),
            ("q8_0", 1555),
            ("q6_k", 1332),
            ("q5_k", 1208),
            ("q4_k", 1092),
        ],
    ),
    (
        "tdt_ctc-1.1b",
        &[
            ("f16", 2430),
            ("q8_0", 1559),
            ("q6_k", 1336),
            ("q5_k", 1212),
            ("q4_k", 1096),
        ],
    ),
];

pub fn catalog() -> Vec<ModelSpec> {
    FAMILIES
        .iter()
        .flat_map(|(family, quantizations)| {
            quantizations.iter().map(move |(quant, size_mb)| ModelSpec {
                family: (*family).to_owned(),
                quant: (*quant).to_owned(),
                file: format!("{family}-{quant}.gguf"),
                size_mb: *size_mb,
                recommended: false,
                installed: false,
            })
        })
        .collect()
}
pub fn find_by_file(file: &str) -> Option<ModelSpec> {
    catalog().into_iter().find(|model| model.file == file)
}

fn catalog_cache_path() -> PathBuf {
    AppConfig::local_data_dir().join("model-catalog.json")
}
fn cached_catalog_files() -> Vec<String> {
    fs::read_to_string(catalog_cache_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default()
}
pub fn refresh_catalog_cache() -> Result<Vec<String>> {
    let raw = reqwest::blocking::get(CATALOG_URL)?
        .error_for_status()?
        .text()?;
    let entries: serde_json::Value = serde_json::from_str(&raw)?;
    let mut files = entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("path").and_then(|value| value.as_str()))
        .filter(|path| path.ends_with(".gguf") && !path.contains('/'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    anyhow::ensure!(
        !files.is_empty(),
        "online model catalog did not contain GGUF files"
    );
    let path = catalog_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(&files)? + "\n")?;
    Ok(files)
}
pub fn catalog_for_config(config: &AppConfig) -> Vec<ModelSpec> {
    let mut models = BTreeMap::<String, ModelSpec>::new();
    for mut model in catalog() {
        model.recommended = is_recommended_for_device(&model.file, &config.inference_device);
        models.insert(model.file.clone(), model);
    }
    for file in cached_catalog_files() {
        let recommended = is_recommended_for_device(&file, &config.inference_device);
        models.entry(file.clone()).or_insert_with(|| ModelSpec {
            family: "online".into(),
            quant: "unknown".into(),
            file,
            size_mb: 0,
            recommended,
            installed: false,
        });
    }
    for file in installed_model_files(config) {
        let recommended = is_recommended_for_device(&file, &config.inference_device);
        models
            .entry(file.clone())
            .and_modify(|model| model.installed = true)
            .or_insert_with(|| ModelSpec {
                family: "local".into(),
                quant: "unknown".into(),
                file,
                size_mb: 0,
                recommended,
                installed: true,
            });
    }
    models.into_values().collect()
}

pub fn installed_model_files(config: &AppConfig) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(config.model_dir_path()) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.ends_with(".gguf") {
                files.push(file);
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

pub fn installed_models(config: &AppConfig) -> Vec<ModelSpec> {
    let mut installed = catalog_for_config(config)
        .into_iter()
        .filter(|model| model.installed)
        .collect::<Vec<_>>();
    installed.sort_by(|a, b| a.file.cmp(&b.file));
    installed
}

pub fn downloadable_models(config: &AppConfig) -> Vec<ModelSpec> {
    let mut models = catalog_for_config(config)
        .into_iter()
        .filter(|model| !model.installed)
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        let a_rank = recommendation_rank(&a.file, &config.inference_device, Some(a.size_mb));
        let b_rank = recommendation_rank(&b.file, &config.inference_device, Some(b.size_mb));
        match (a_rank, b_rank) {
            (Some(a_rank), Some(b_rank)) => a_rank
                .cmp(&b_rank)
                .then_with(|| a.size_mb.cmp(&b.size_mb))
                .then_with(|| a.file.cmp(&b.file)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.size_mb.cmp(&b.size_mb).then_with(|| a.file.cmp(&b.file)),
        }
    });
    models
}

pub fn recommended_model_for_device(device: &InferenceDevice) -> &'static str {
    match device.effective() {
        InferenceDevice::NvidiaGpu => "tdt_ctc-110m-f16.gguf",
        InferenceDevice::Cpu => "tdt_ctc-110m-q4_k.gguf",
        InferenceDevice::Auto => unreachable!("auto must resolve before model recommendation"),
    }
}

pub fn is_recommended_for_device(file: &str, device: &InferenceDevice) -> bool {
    recommendation_rank(file, device, known_size_mb(file)).is_some()
}

fn known_size_mb(file: &str) -> Option<u32> {
    find_by_file(file).map(|model| model.size_mb)
}

fn recommendation_rank(file: &str, device: &InferenceDevice, size_mb: Option<u32>) -> Option<u8> {
    let file = file.to_ascii_lowercase();
    let preferred_family =
        file.starts_with("tdt_ctc-110m") || file.starts_with("realtime_eou_120m-v1");
    match device.effective() {
        InferenceDevice::NvidiaGpu => {
            if file.ends_with("-f16.gguf") && preferred_family && size_mb.unwrap_or(u32::MAX) <= 300
            {
                Some(if file.starts_with("tdt_ctc-110m") {
                    0
                } else {
                    1
                })
            } else {
                None
            }
        }
        InferenceDevice::Cpu => {
            let compact_quant = file.ends_with("-q4_k.gguf") || file.ends_with("-q5_k.gguf");
            if compact_quant && preferred_family && size_mb.unwrap_or(u32::MAX) <= 160 {
                Some(if file.starts_with("tdt_ctc-110m") {
                    0
                } else {
                    1
                })
            } else {
                None
            }
        }
        InferenceDevice::Auto => unreachable!("auto must resolve before model ranking"),
    }
}

pub fn download_model<F>(config: &AppConfig, filename: &str, mut on_progress: F) -> Result<PathBuf>
where
    F: FnMut(u64, Option<u64>),
{
    validate_model_filename(filename)?;
    let spec = catalog_for_config(config)
        .into_iter()
        .find(|model| model.file == filename)
        .with_context(|| format!("unknown cached or local model: {filename}"))?;
    let dir = config.model_dir_path();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let target = dir.join(&spec.file);
    if target.exists() {
        return Ok(target);
    }
    let partial = target.with_extension(format!("gguf.partial.{:016x}", rand::random::<u64>()));
    let url = format!("{BASE_URL}/{}", spec.file);
    tracing::info!(%url, target = %target.display(), partial = %partial.display(), "model download begin");
    let result = (|| -> Result<u64> {
        let mut response = reqwest::blocking::get(&url)?.error_for_status()?;
        let total = response.content_length();
        let mut output = fs::File::create(&partial)
            .with_context(|| format!("creating {}", partial.display()))?;
        let mut buffer = [0_u8; 128 * 1024];
        let mut downloaded = 0_u64;
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
            downloaded += count as u64;
            on_progress(downloaded, total);
        }
        output.flush()?;
        output.sync_all()?;
        replace_file_atomic(&partial, &target)
            .with_context(|| format!("renaming partial model to {}", target.display()))?;
        Ok(downloaded)
    })();
    match result {
        Ok(downloaded) => {
            tracing::info!(target = %target.display(), bytes = downloaded, "model download complete");
            Ok(target)
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

pub fn smoke_audio_path() -> PathBuf {
    crate::config::runtime_root()
        .join("fixtures")
        .join("parakeet-smoke.wav")
}
pub fn ensure_smoke_audio(path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.exists(),
        "smoke-test WAV is missing: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_contains_default() {
        assert!(find_by_file("tdt_ctc-110m-f16.gguf").is_some());
    }
    #[test]
    fn catalog_preserves_legacy_families() {
        for file in [
            "tdt-0.6b-v2-f16.gguf",
            "tdt-1.1b-f16.gguf",
            "tdt_ctc-1.1b-f16.gguf",
        ] {
            assert!(find_by_file(file).is_some(), "missing {file}");
        }
    }
    #[test]
    fn gpu_and_cpu_recommendations_are_device_specific() {
        assert!(is_recommended_for_device(
            "tdt_ctc-110m-f16.gguf",
            &InferenceDevice::NvidiaGpu
        ));
        assert!(is_recommended_for_device(
            "tdt_ctc-110m-q4_k.gguf",
            &InferenceDevice::Cpu
        ));
        assert!(!is_recommended_for_device(
            "tdt_ctc-110m-f16.gguf",
            &InferenceDevice::Cpu
        ));
    }
    #[test]
    fn only_approved_names_are_downloadable() {
        assert!(find_by_file("..\\evil.gguf").is_none());
    }
}
