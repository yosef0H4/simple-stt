use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::parakeet_native::ParakeetNative;

pub const REPO_ID: &str = "mudler/parakeet-cpp-gguf";
const BASE_URL: &str = "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main";

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
            ("q5_k", 1213),
            ("q4_k", 1097),
        ],
    ),
];

pub fn catalog() -> Vec<ModelSpec> {
    let mut values = Vec::new();
    for (family, quants) in FAMILIES {
        for (quant, size_mb) in *quants {
            values.push(ModelSpec {
                family,
                quant,
                file: file_name(family, quant),
                size_mb: *size_mb,
                recommended: *quant == "f16",
            });
        }
    }
    values
}

fn file_name(family: &str, quant: &str) -> String {
    format!("{family}-{quant}.gguf")
}

pub fn find_by_file(file: &str) -> Option<ModelSpec> {
    catalog().into_iter().find(|model| model.file == file)
}

pub fn local_model_path(file: &str) -> PathBuf {
    AppConfig::model_store_dir().join(file)
}

pub fn download_model(file: &str) -> Result<PathBuf> {
    let spec = find_by_file(file).with_context(|| format!("unknown approved model: {file}"))?;
    fs::create_dir_all(AppConfig::model_store_dir())?;
    let target = local_model_path(&spec.file);
    if target.exists() {
        tracing::info!(target = %target.display(), "model file already exists locally; skipping download");
        return Ok(target);
    }
    let partial = target.with_extension("gguf.partial");
    let url = format!("{BASE_URL}/{}", spec.file);
    tracing::info!(%url, target = %target.display(), "downloading model");
    let mut response = reqwest::blocking::get(&url)?.error_for_status()?;
    let mut out = fs::File::create(&partial)?;
    let mut buf = [0_u8; 1024 * 128];
    let mut downloaded = 0_u64;
    loop {
        let read = response.read(&mut buf)?;
        if read == 0 {
            break;
        }
        out.write_all(&buf[..read])?;
        let prev_mb = downloaded / (10 * 1024 * 1024);
        downloaded += read as u64;
        let curr_mb = downloaded / (10 * 1024 * 1024);
        if curr_mb > prev_mb {
            tracing::info!(downloaded, "downloaded model bytes");
        }
    }
    out.flush()?;
    fs::rename(&partial, &target)?;
    tracing::info!(target = %target.display(), "model download complete");
    Ok(target)
}

pub fn smoke_test_model(runtime_dir: &Path, model_path: &Path, audio: &Path) -> Result<String> {
    let engine = ParakeetNative::load(runtime_dir, model_path)?;
    engine.transcribe_wav(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_current_default_model() {
        assert!(find_by_file("tdt_ctc-110m-f16.gguf").is_some());
    }

    #[test]
    fn f16_models_are_marked_recommended() {
        let values = catalog();
        assert!(values.iter().any(|model| model.family == "tdt-0.6b-v3"
            && model.quant == "f16"
            && model.recommended));
        assert!(values.iter().any(|model| !model.recommended));
    }
}
