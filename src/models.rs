use crate::config::{replace_file_atomic, validate_model_filename, AppConfig};
use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const BASE_URL: &str = "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main";

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub family: &'static str,
    pub quant: &'static str,
    pub file: String,
    pub size_mb: u32,
    pub recommended: bool,
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
                family,
                quant,
                file: format!("{family}-{quant}.gguf"),
                size_mb: *size_mb,
                recommended: *quant == "f16",
            })
        })
        .collect()
}
pub fn find_by_file(file: &str) -> Option<ModelSpec> {
    catalog().into_iter().find(|model| model.file == file)
}

pub fn download_model<F>(config: &AppConfig, filename: &str, mut on_progress: F) -> Result<PathBuf>
where
    F: FnMut(u64, Option<u64>),
{
    validate_model_filename(filename)?;
    let spec =
        find_by_file(filename).with_context(|| format!("unknown approved model: {filename}"))?;
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
    fn every_f16_variant_remains_recommended() {
        assert!(catalog()
            .into_iter()
            .filter(|model| model.quant == "f16")
            .all(|model| model.recommended));
    }
    #[test]
    fn only_approved_names_are_downloadable() {
        assert!(find_by_file("..\\evil.gguf").is_none());
    }
}
