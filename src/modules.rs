//! Installed module inventory and action-time requirements; no native libraries are loaded.
use crate::config::{AppConfig, ModelSelectionMode};
use reader_core::routing;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;
pub fn stt_runtime(config: &AppConfig, language: &str) -> Option<PathBuf> {
    if config.speech.selection_mode == ModelSelectionMode::FollowKeyboard {
        return config
            .speech
            .language_runtimes
            .get(language)
            .and_then(|id| id.as_ref())
            .and_then(|id| {
                #[cfg(target_os = "windows")]
                if id == "parakeet-vulkan" {
                    return Some(config.resolve_from_runtime_root(
                        "external/parakeet-runtime/parakeet-windows-vulkan",
                    ));
                }
                routing::registry()
                    .runtimes
                    .into_iter()
                    .find(|runtime| runtime.id == *id && runtime.feature == routing::Feature::Stt)
                    .and_then(|runtime| runtime.asset.map(|asset| crate::assets::directory(&asset)))
            });
    }
    (!config.speech.runtime_dir.is_empty()).then(|| config.parakeet_runtime_dir_path())
}
pub fn runtime_ready(path: Option<&std::path::Path>) -> bool {
    path.is_some_and(|path| {
        crate::config::parakeet_native_library_candidates(path)
            .iter()
            .any(|p| p.is_file())
    })
}
pub fn requirement_message(
    feature: &str,
    runtime: bool,
    model: bool,
    language: Option<&str>,
) -> String {
    use crate::localization::{current, translate};
    let locale = current();
    let feature = translate(locale, &format!("module.{feature}"), &[]);
    let runtime = translate(
        locale,
        if runtime {
            "module.ready"
        } else {
            "module.missing"
        },
        &[],
    );
    let model = translate(
        locale,
        if model {
            "module.ready"
        } else {
            "module.missing"
        },
        &[],
    );
    translate(
        locale,
        "module.requirements",
        &[
            ("feature", feature.as_str()),
            ("runtime", runtime.as_str()),
            ("model", model.as_str()),
            ("language", language.unwrap_or("")),
        ],
    )
}
#[derive(Clone, Copy, Debug, Default)]
struct Hardware {
    nvidia_cuda13: bool,
    gpu: bool,
}

// Detection is deliberately conservative and happens only once per settings process.
// Driver and GPU capability are necessary signals, not proof that a package will load.
fn hardware() -> Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    *HARDWARE.get_or_init(detect_hardware)
}

// CUDA 13 and cuDNN 9 require at least this Linux driver and Turing (7.5).
// This does not guarantee the package contains a compatible kernel for every GPU.
#[cfg(any(target_os = "linux", test))]
fn cuda13_compatible_devices(output: &str) -> bool {
    output.lines().any(|line| {
        let Some((driver, capability)) = line.split_once(',') else {
            return false;
        };
        let mut driver = driver.trim().split('.').map(str::parse::<u32>);
        let version = match (driver.next(), driver.next(), driver.next()) {
            (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch))) => (major, minor, patch),
            _ => return false,
        };
        let Some((major, minor)) = capability.trim().split_once('.') else {
            return false;
        };
        let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
            return false;
        };
        version >= (580, 65, 6) && (major, minor) >= (7, 5)
    })
}

fn detect_hardware() -> Hardware {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let nvidia_cuda13 = Command::new("nvidia-smi")
            .args([
                "--query-gpu=driver_version,compute_cap",
                "--format=csv,noheader,nounits",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .is_some_and(|mut child| {
                let deadline = Instant::now() + Duration::from_millis(500);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) if status.success() => {
                            let mut output = String::new();
                            break child.stdout.take().is_some_and(|out| {
                                out.take(65536).read_to_string(&mut output).is_ok()
                                    && cuda13_compatible_devices(&output)
                            });
                        }
                        Ok(None) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        _ => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break false;
                        }
                    }
                }
            });
        // A bound DRM device does not prove the Vulkan features required by inference.
        let gpu = nvidia_cuda13
            || std::fs::read_dir("/sys/class/drm")
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.strip_prefix("card").is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                    }) && entry.path().join("device/driver").exists()
                });
        Hardware { nvidia_cuda13, gpu }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Hardware::default()
    }
}

fn runtime_recommendation(id: &str, hardware: Hardware) -> Option<&'static str> {
    match id {
        "parakeet-cuda" | "sherpa-cuda" if hardware.nvidia_cuda13 => Some("nvidia_driver_detected"),
        "parakeet-vulkan" if hardware.gpu && !hardware.nvidia_cuda13 => Some("gpu_available"),
        "sherpa-cpu" if !hardware.nvidia_cuda13 => Some("cpu_voice"),
        "paddle-cpu" => Some("small_capture_cpu"),
        _ => None,
    }
}

fn model_recommendation(id: &str, usable_nvidia_voice: bool) -> Option<&'static str> {
    match id {
        "kokoro" if usable_nvidia_voice => Some("nvidia_voice"),
        "supertonic3" if usable_nvidia_voice => Some("multilingual_voice"),
        "supertonic3" => Some("cpu_voice"),
        "paddle-ar-en" => Some("available_ocr"),
        _ => None,
    }
}

pub fn inventory(config: &AppConfig) -> Value {
    let registry = routing::registry();
    let hardware = hardware();
    let usable_nvidia_voice = hardware.nvidia_cuda13
        && registry.runtimes.iter().any(|runtime| {
            runtime.id == "sherpa-cuda"
                && routing::runtime_installed(runtime, &crate::assets::root())
        });
    let runtimes=registry.runtimes.iter().map(|runtime| {
        let directory=runtime.asset.as_ref().map(|id|crate::assets::directory(id));
        let installed=reader_core::routing::runtime_installed(runtime,&crate::assets::root());
        #[cfg(target_os = "windows")]
        let (directory,installed) = if runtime.id == "parakeet-vulkan" {
            let bundled = config.resolve_from_runtime_root("external/parakeet-runtime/parakeet-windows-vulkan");
            let ready = runtime_ready(Some(&bundled));
            (Some(bundled), ready)
        } else { (directory, installed) };
        let recommendation = runtime_recommendation(&runtime.id, hardware);
        json!({"id":runtime.id,"feature":runtime.feature,"engine":runtime.engine,"backend":runtime.backend,"label":runtime.label,"installed":installed,"directory":directory,"asset":runtime.asset,"recommended":recommendation.is_some(),"recommendation":recommendation})
    }).collect::<Vec<_>>();
    let mut models=registry.models.iter().map(|model| {
        let recommendation = model_recommendation(&model.id, usable_nvidia_voice);
        json!({"id":model.id,"feature":model.feature,"engine":model.engine,"label":model.label,"languages":model.languages,"installed":routing::asset_installed(&model.asset,&crate::assets::root()),"asset":model.asset,"recommended":recommendation.is_some(),"recommendation":recommendation})
    }).collect::<Vec<_>>();
    models.extend(crate::models::catalog_for_config(config).iter().map(|model| {
        let languages=model.languages.iter().filter_map(|label|label.rsplit_once('(').map(|(_,id)|id.trim_end_matches(')').to_owned())).collect::<Vec<_>>();
        json!({"id":model.file,"feature":"stt","engine":"parakeet","label":format!("{} · {}",model.family,model.quant),"languages":languages,"installed":model.installed,"recommended":model.recommended,"recommendation":model.recommended.then_some("speech_model"),"asset":null})
    }));
    json!({"runtimes":runtimes,"models":models})
}

/// Immutable snapshot used by completion-only automatic assignment.
#[derive(Clone)]
pub struct AutoInventory {
    pub registry: routing::Registry,
    pub installed: std::collections::BTreeSet<String>,
    pub stt_models: Vec<crate::models::ModelSpec>,
    pub nvidia_cuda13: bool,
    pub gpu: bool,
}
impl AutoInventory {
    pub fn capture(config: &AppConfig) -> Self {
        let registry = routing::registry();
        let mut installed = std::collections::BTreeSet::new();
        for asset in crate::assets::catalog() {
            if crate::assets::installed_at(&crate::assets::root(), &asset) {
                installed.insert(asset.id);
            }
        }
        #[cfg(target_os = "windows")]
        if runtime_ready(Some(&config.resolve_from_runtime_root(
            "external/parakeet-runtime/parakeet-windows-vulkan",
        ))) {
            installed.insert("parakeet-vulkan".into());
        }
        let hardware = hardware();
        Self {
            registry,
            installed,
            stt_models: crate::models::installed_models(config),
            nvidia_cuda13: hardware.nvidia_cuda13,
            gpu: hardware.gpu,
        }
    }
    fn has_runtime(&self, runtime: &routing::Runtime) -> bool {
        runtime
            .asset
            .as_ref()
            .is_some_and(|asset| self.installed.contains(asset))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoChanges {
    pub stt: bool,
    pub reader: bool,
}

/// Apply only after a user-requested download completed successfully.
pub fn auto_select(
    stt: &mut AppConfig,
    reader: &mut reader_core::AppConfig,
    completed: &str,
    languages: &[String],
    inventory: &AutoInventory,
) -> AutoChanges {
    let mut result = AutoChanges::default();
    let speech_event = inventory
        .stt_models
        .iter()
        .any(|model| model.file == completed)
        || inventory.registry.runtimes.iter().any(|runtime| {
            runtime.feature == routing::Feature::Stt && runtime.asset.as_deref() == Some(completed)
        });
    if speech_event {
        result.stt = auto_select_stt(stt, completed, languages, inventory);
    }
    for feature in [routing::Feature::Tts, routing::Feature::Ocr] {
        let relevant = inventory.registry.runtimes.iter().any(|runtime| {
            runtime.feature == feature && runtime.asset.as_deref() == Some(completed)
        }) || inventory
            .registry
            .models
            .iter()
            .any(|model| model.feature == feature && model.asset == completed);
        if !relevant {
            continue;
        }
        let selection = if feature == routing::Feature::Tts {
            &mut reader.speech.selection
        } else {
            &mut reader.ocr.selection
        };
        result.reader |= auto_select_reader(selection, feature, completed, languages, inventory);
    }
    result
}

/// Merge completion selections into the latest on-disk configurations.
pub fn persist_auto_select(completed: &str) -> anyhow::Result<AutoChanges> {
    let _guard = crate::config::lock_config()?;
    let mut stt = AppConfig::load()?;
    let languages = crate::capture::input_language::keyboard_languages()
        .languages
        .into_iter()
        .map(|language| language.id)
        .collect::<Vec<_>>();
    let inventory = AutoInventory::capture(&stt);
    let mut reader = crate::reader::load()?;
    let changes = auto_select(&mut stt, &mut reader, completed, &languages, &inventory);
    if changes.stt {
        stt.save()?;
    }
    if changes.reader {
        for attempt in 0..3 {
            let revision = reader.revision;
            match crate::reader::store().save(reader.clone(), revision) {
                Ok(_) => return Ok(changes),
                Err(error) if attempt < 2 && error.to_string().contains("revision conflict") => {
                    reader = crate::reader::load()?;
                    let mut latest_stt = stt.clone();
                    if !auto_select(
                        &mut latest_stt,
                        &mut reader,
                        completed,
                        &languages,
                        &inventory,
                    )
                    .reader
                    {
                        return Ok(AutoChanges {
                            stt: changes.stt,
                            reader: false,
                        });
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(changes)
}

fn auto_select_stt(
    config: &mut AppConfig,
    completed: &str,
    languages: &[String],
    inventory: &AutoInventory,
) -> bool {
    let mut runtimes: Vec<_> = inventory
        .registry
        .runtimes
        .iter()
        .filter(|r| r.feature == routing::Feature::Stt && inventory.has_runtime(r))
        .collect();
    let hardware = Hardware {
        nvidia_cuda13: inventory.nvidia_cuda13,
        gpu: inventory.gpu,
    };
    runtimes.sort_by_key(|r| {
        (
            r.asset.as_deref() != Some(completed),
            runtime_recommendation(&r.id, hardware).is_none(),
        )
    });
    let mut models: Vec<_> = inventory
        .stt_models
        .iter()
        .filter(|m| m.installed && !m.languages.is_empty())
        .collect();
    models.sort_by_key(|m| (m.file != completed, !m.recommended));
    let mut changed = false;
    if config.speech.selection_mode == ModelSelectionMode::SingleModel {
        let Some(runtime) = runtimes.iter().find(|runtime| {
            config.speech.runtime_dir.is_empty()
                || runtime.asset.as_ref().is_some_and(|asset| {
                    config.parakeet_runtime_dir_path() == crate::assets::directory(asset)
                })
        }) else {
            return false;
        };
        let model = models.iter().find(|model| {
            config
                .speech
                .single_model_filename
                .as_ref()
                .is_none_or(|existing| existing == &model.file)
        });
        if let Some(model) = model {
            if config.speech.single_model_filename.is_none() {
                config.speech.single_model_filename = Some(model.file.clone());
                changed = true;
            }
            if config.speech.runtime_dir.is_empty() {
                if let Some(asset) = &runtime.asset {
                    #[cfg(target_os = "windows")]
                    let path = if runtime.id == "parakeet-vulkan" {
                        config.resolve_from_runtime_root(
                            "external/parakeet-runtime/parakeet-windows-vulkan",
                        )
                    } else {
                        crate::assets::directory(asset)
                    };
                    #[cfg(not(target_os = "windows"))]
                    let path = crate::assets::directory(asset);
                    config.speech.runtime_dir = path.to_string_lossy().into_owned();
                    changed = true;
                }
            }
        }
    } else {
        for language in languages {
            let runtime = runtimes.iter().find(|runtime| {
                config
                    .speech
                    .language_runtimes
                    .get(language)
                    .and_then(Option::as_ref)
                    .is_none_or(|existing| existing == &runtime.id)
            });
            let model = models.iter().find(|model| {
                config
                    .speech
                    .language_models
                    .get(language)
                    .and_then(Option::as_ref)
                    .is_none_or(|existing| existing == &model.file)
                    && model.languages.iter().any(|label| {
                        label
                            .rsplit_once('(')
                            .is_some_and(|(_, id)| id.trim_end_matches(')') == language)
                    })
            });
            if let (Some(runtime), Some(model)) = (runtime, model) {
                if config
                    .speech
                    .language_models
                    .get(language)
                    .is_none_or(Option::is_none)
                {
                    config
                        .speech
                        .language_models
                        .insert(language.clone(), Some(model.file.clone()));
                    changed = true;
                }
                if config
                    .speech
                    .language_runtimes
                    .get(language)
                    .is_none_or(Option::is_none)
                {
                    config
                        .speech
                        .language_runtimes
                        .insert(language.clone(), Some(runtime.id.clone()));
                    changed = true;
                }
            }
        }
    }
    changed
}

fn auto_select_reader(
    selection: &mut routing::Selection,
    feature: routing::Feature,
    completed: &str,
    languages: &[String],
    inventory: &AutoInventory,
) -> bool {
    let mut changed = false;
    match selection.mode {
        routing::SelectionMode::SingleModel => {
            changed |=
                fill_reader_binding(&mut selection.single, feature, None, completed, inventory)
        }
        routing::SelectionMode::FollowKeyboard | routing::SelectionMode::AutoDetect => {
            let mut targets: std::collections::BTreeSet<String> =
                languages.iter().map(|l| routing::primary(l)).collect();
            targets.extend(selection.languages.keys().cloned());
            for language in targets {
                let mut binding = selection
                    .languages
                    .get(&language)
                    .cloned()
                    .unwrap_or_default();
                if fill_reader_binding(&mut binding, feature, Some(&language), completed, inventory)
                {
                    selection.languages.insert(language, binding);
                    changed = true;
                }
            }
        }
    }
    changed
}

fn fill_reader_binding(
    binding: &mut routing::Binding,
    feature: routing::Feature,
    language: Option<&str>,
    completed: &str,
    inventory: &AutoInventory,
) -> bool {
    let mut runtimes: Vec<_> = inventory
        .registry
        .runtimes
        .iter()
        .filter(|runtime| {
            runtime.feature == feature
                && inventory.has_runtime(runtime)
                && binding.runtime.as_ref().is_none_or(|id| id == &runtime.id)
                && (binding.device != routing::Device::Gpu || runtime.backend != "cpu")
        })
        .collect();
    let hardware = Hardware {
        nvidia_cuda13: inventory.nvidia_cuda13,
        gpu: inventory.gpu,
    };
    runtimes.sort_by_key(|runtime| {
        (
            runtime.asset.as_deref() != Some(completed),
            runtime_recommendation(&runtime.id, hardware).is_none(),
        )
    });
    let mut models: Vec<_> = inventory
        .registry
        .models
        .iter()
        .filter(|model| {
            model.feature == feature
                && inventory.installed.contains(&model.asset)
                && binding.model.as_ref().is_none_or(|id| id == &model.id)
                && language.is_none_or(|language| {
                    model.languages.is_empty() || model.languages.iter().any(|l| l == language)
                })
        })
        .collect();
    let nvidia_installed = hardware.nvidia_cuda13
        && inventory
            .registry
            .runtimes
            .iter()
            .any(|r| r.id == "sherpa-cuda" && inventory.has_runtime(r));
    models.sort_by_key(|model| {
        (
            model.asset != completed,
            model_recommendation(&model.id, nvidia_installed).is_none(),
        )
    });
    for runtime in runtimes {
        for model in &models {
            if runtime.engine != model.engine {
                continue;
            }
            let mut candidate = binding.clone();
            if candidate.runtime.is_none() {
                candidate.runtime = Some(runtime.id.clone());
            }
            if candidate.model.is_none() {
                candidate.model = Some(model.id.clone());
            }
            let test = routing::Selection {
                single: candidate.clone(),
                ..Default::default()
            };
            if test.validate(feature).is_ok() && candidate != *binding {
                *binding = candidate;
                return true;
            }
        }
    }
    false
}

pub fn stt_model_ready(config: &AppConfig, language: &str) -> bool {
    let Some(path) = config.model_path_for(language).filter(|p| p.is_file()) else {
        return false;
    };
    if config.speech.selection_mode == ModelSelectionMode::SingleModel {
        return true;
    }
    crate::models::catalog_for_config(config)
        .iter()
        .find(|model| {
            path.file_name()
                .is_some_and(|name| name == model.file.as_str())
        })
        .is_none_or(|model| {
            model.languages.is_empty()
                || model.languages.iter().any(|label| {
                    label
                        .rsplit_once('(')
                        .is_some_and(|(_, id)| id.trim_end_matches(')') == language)
                })
        })
}

#[cfg(test)]
mod recommendation_tests {
    use super::*;

    fn inventory_fixture() -> AutoInventory {
        let mut stt_models = crate::models::catalog();
        for model in &mut stt_models {
            model.installed = model.file == "tdt_ctc-110m-q8_0.gguf"
                || model.file == "lemura-arabic-asr-lite-q8_0.gguf";
        }
        AutoInventory {
            registry: routing::registry(),
            installed: [
                "parakeet-vulkan",
                "sherpa-cpu",
                "kokoro",
                "supertonic3",
                "paddle-ar-en",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            stt_models,
            nvidia_cuda13: false,
            gpu: true,
        }
    }

    #[test]
    fn completion_fills_only_compatible_empty_speech_bindings() {
        let inventory = inventory_fixture();
        let mut stt = AppConfig::default();
        let mut reader = reader_core::AppConfig::default();
        let languages = vec!["en".into(), "ar".into()];
        let changed = auto_select(
            &mut stt,
            &mut reader,
            "parakeet-vulkan",
            &languages,
            &inventory,
        );
        assert!(changed.stt);
        assert_eq!(
            stt.speech.language_models["en"].as_deref(),
            Some("tdt_ctc-110m-q8_0.gguf")
        );
        assert_eq!(
            stt.speech.language_models["ar"].as_deref(),
            Some("lemura-arabic-asr-lite-q8_0.gguf")
        );
        stt.speech
            .language_models
            .insert("en".into(), Some("custom.gguf".into()));
        let before = stt.clone();
        auto_select(
            &mut stt,
            &mut reader,
            "tdt_ctc-110m-q8_0.gguf",
            &languages,
            &inventory,
        );
        assert_eq!(stt, before);
    }

    #[test]
    fn completion_preserves_reader_choices_and_checks_language_and_device() {
        let inventory = inventory_fixture();
        let mut stt = AppConfig::default();
        let mut reader = reader_core::AppConfig::default();
        reader
            .speech
            .selection
            .languages
            .entry("en".into())
            .or_default()
            .voice = Some("af_bella".into());
        let changed = auto_select(
            &mut stt,
            &mut reader,
            "kokoro",
            &["ar".into(), "en".into()],
            &inventory,
        );
        assert!(changed.reader);
        assert_eq!(
            reader.speech.selection.languages["en"].model.as_deref(),
            Some("kokoro")
        );
        assert_eq!(
            reader.speech.selection.languages["en"].voice.as_deref(),
            Some("af_bella")
        );
        assert_eq!(
            reader.speech.selection.languages["ar"].model.as_deref(),
            Some("supertonic3")
        );
        assert_eq!(reader.speech.selection.languages.len(), 2);
        let before = reader.clone();
        auto_select(&mut stt, &mut reader, "unrelated", &[], &inventory);
        assert_eq!(reader, before);
        let mut binding = routing::Binding {
            device: routing::Device::Gpu,
            ..Default::default()
        };
        assert!(!fill_reader_binding(
            &mut binding,
            routing::Feature::Tts,
            Some("en"),
            "kokoro",
            &inventory
        ));
    }

    #[test]
    fn runtime_policy_prefers_detected_acceleration_and_cpu_for_small_ocr() {
        let nvidia = Hardware {
            nvidia_cuda13: true,
            gpu: true,
        };
        let gpu = Hardware {
            nvidia_cuda13: false,
            gpu: true,
        };
        let unknown = Hardware::default();
        assert_eq!(
            runtime_recommendation("parakeet-cuda", nvidia),
            Some("nvidia_driver_detected")
        );
        assert_eq!(runtime_recommendation("parakeet-vulkan", nvidia), None);
        assert_eq!(
            runtime_recommendation("parakeet-vulkan", gpu),
            Some("gpu_available")
        );
        assert_eq!(runtime_recommendation("parakeet-cuda", gpu), None);
        assert_eq!(runtime_recommendation("parakeet-vulkan", unknown), None);
        assert_eq!(
            runtime_recommendation("sherpa-cpu", unknown),
            Some("cpu_voice")
        );
        assert_eq!(runtime_recommendation("sherpa-cpu", nvidia), None);
        assert_eq!(
            runtime_recommendation("paddle-cpu", nvidia),
            Some("small_capture_cpu")
        );
        assert_eq!(runtime_recommendation("paddle-cuda", nvidia), None);
    }

    #[test]
    fn cuda13_requires_a_supported_driver_and_gpu() {
        assert!(cuda13_compatible_devices("580.65.06, 7.5\n"));
        assert!(cuda13_compatible_devices("590.01.01, 8.9\n"));
        assert!(!cuda13_compatible_devices("580.65.05, 8.9\n"));
        assert!(!cuda13_compatible_devices("579.99.99, 8.9\n"));
        assert!(!cuda13_compatible_devices("580.65.06, 7.0\n"));
        assert!(!cuda13_compatible_devices("580.65.06, [N/A]\n"));
        assert!(!cuda13_compatible_devices("unknown, 8.9\n"));
        assert!(cuda13_compatible_devices(
            "580.65.06, 7.0\n580.65.06, 8.0\n"
        ));
    }

    #[test]
    fn voice_recommendations_depend_on_installed_nvidia_runtime() {
        assert_eq!(
            model_recommendation("supertonic3", false),
            Some("cpu_voice")
        );
        assert_eq!(model_recommendation("kokoro", false), None);
        assert_eq!(model_recommendation("kokoro", true), Some("nvidia_voice"));
        assert_eq!(
            model_recommendation("supertonic3", true),
            Some("multilingual_voice")
        );
        assert_eq!(model_recommendation("unknown", true), None);
    }
}
