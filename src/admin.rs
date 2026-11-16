//! Headless administration using the same stores, validation and asset manager as Settings.
use anyhow::{bail, Context, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const MAX_SECRET_BYTES: usize = 1024 * 1024;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect or edit the complete JSON configuration without starting a model.
    Config {
        #[arg(long, value_enum, default_value = "app", global = true)]
        scope: Scope,
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Discover stable model/runtime IDs, install their dependencies, or select them.
    Assets {
        #[command(subcommand)]
        command: AssetCommand,
    },
    /// Store provider credentials in the operating-system credential vault.
    Credentials {
        #[command(subcommand)]
        command: CredentialCommand,
    },
    /// Run the installed headless API controller (no audio device or desktop shell).
    Serve,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Scope {
    App,
    Reader,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Path,
    Show,
    /// Show all default values, including optional settings absent from an older file.
    Defaults,
    Get {
        key: String,
    },
    /// Set a dot-separated key to a JSON value, e.g. server.enabled true.
    Set {
        key: String,
        value: String,
    },
    /// Validate and import a complete JSON document. Reader revisions are checked.
    Import {
        path: PathBuf,
    },
    /// Open the JSON file in VISUAL/EDITOR. Models and runtimes are listed by assets list.
    Edit,
}

#[derive(Debug, Subcommand)]
pub enum AssetCommand {
    /// JSON inventory includes stable IDs, dependencies, install status and assignments.
    List,
    /// Wait for verified installation, including dependencies. Ctrl+C cancels safely.
    Install {
        id: String,
    },
    Remove {
        id: String,
    },
    /// Assign installed model/runtime IDs. Omit --language to select single-model mode.
    Select {
        #[arg(value_enum)]
        feature: Feature,
        #[arg(long)]
        model: String,
        #[arg(long)]
        runtime: String,
        #[arg(long)]
        language: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum CredentialCommand {
    /// Print the shared server token, creating and saving one when absent.
    Generate {
        #[arg(value_enum)]
        kind: GeneratedCredentialKind,
        /// Rotate the saved token. Restart a running server afterward.
        #[arg(long)]
        replace: bool,
        /// Generate a fresh token without a keyring or saving it (for headless hosts).
        #[arg(long, conflicts_with = "replace")]
        stdout: bool,
    },
    /// Report whether credentials are present without revealing their values.
    List,
    /// Read a credential from stdin or a named environment variable.
    Set {
        #[arg(value_enum)]
        kind: CredentialKind,
        #[arg(long, conflicts_with = "env")]
        stdin: bool,
        #[arg(long, value_name = "NAME")]
        env: Option<String>,
    },
    Remove {
        #[arg(value_enum)]
        kind: RemovableCredentialKind,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GeneratedCredentialKind {
    Server,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CredentialKind {
    Ai,
    Stt,
    Speech,
    Ocr,
    Server,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RemovableCredentialKind {
    Ai,
    Stt,
    Speech,
    Ocr,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Feature {
    Stt,
    Tts,
    Ocr,
}

pub fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Config { scope, command } => config(scope, command),
        Command::Assets { command } => assets(command),
        Command::Credentials { command } => credentials(command),
        Command::Serve => {
            crate::reader::initialize()?;
            let executable = std::env::current_exe()?.with_file_name(if cfg!(windows) {
                "readerd.exe"
            } else {
                "readerd"
            });
            let status = std::process::Command::new(executable)
                .args(["--config"])
                .arg(crate::reader::store().path())
                .arg("server")
                .env("SIMPLE_STT_EMBEDDED_READER", "1")
                .env("SIMPLE_STT_READER_DATA", crate::reader::data_dir())
                .status()
                .context("starting installed API controller")?;
            anyhow::ensure!(status.success(), "API controller exited with {status}");
            Ok(())
        }
    }
}

fn print(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn pointer(key: &str) -> String {
    if key.starts_with('/') {
        return key.to_owned();
    }
    format!(
        "/{}",
        key.split('.')
            .map(|part| part.replace('~', "~0").replace('/', "~1"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

fn pointer_parts(key: &str) -> Result<Vec<String>> {
    let parts = if let Some(pointer) = key.strip_prefix('/') {
        pointer
            .split('/')
            .map(|part| {
                let mut decoded = String::new();
                let mut chars = part.chars();
                while let Some(ch) = chars.next() {
                    if ch == '~' {
                        match chars.next() {
                            Some('0') => decoded.push('~'),
                            Some('1') => decoded.push('/'),
                            _ => bail!("invalid JSON Pointer escape in key {key:?}"),
                        }
                    } else {
                        decoded.push(ch);
                    }
                }
                Ok(decoded)
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        key.split('.').map(str::to_owned).collect()
    };
    anyhow::ensure!(
        !parts.is_empty() && parts.iter().all(|part| !part.is_empty()),
        "configuration key cannot contain empty path components"
    );
    Ok(parts)
}

fn set_value(root: &mut Value, key: &str, value: Value) -> Result<()> {
    let parts = pointer_parts(key)?;
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        current = match current {
            Value::Object(object) => object
                .get_mut(part)
                .with_context(|| format!("unknown configuration key component {part:?}"))?,
            Value::Array(array) => array
                .get_mut(
                    part.parse::<usize>()
                        .context("array index must be an integer")?,
                )
                .with_context(|| format!("unknown array index {part:?}"))?,
            _ => bail!("configuration key does not point to an object or array"),
        };
    }
    let last = parts.last().expect("non-empty key");
    match current {
        Value::Object(object) => {
            object.insert(last.clone(), value);
        }
        Value::Array(array) => {
            let index = last
                .parse::<usize>()
                .context("array index must be an integer")?;
            *array
                .get_mut(index)
                .with_context(|| format!("unknown array index {last:?}"))? = value;
        }
        _ => bail!("configuration key does not point to an object or array"),
    }
    Ok(())
}

fn load_app_config() -> Result<crate::config::AppConfig> {
    if crate::config::AppConfig::config_path().exists() {
        crate::config::AppConfig::load()
    } else {
        Ok(crate::config::AppConfig::default())
    }
}

fn config(scope: Scope, command: ConfigCommand) -> Result<()> {
    let path = match scope {
        Scope::App => crate::config::AppConfig::config_path(),
        Scope::Reader => crate::reader::store().path().to_owned(),
    };
    if matches!(command, ConfigCommand::Path) {
        return print(&json!(path));
    }
    if matches!(command, ConfigCommand::Defaults) {
        return print(&match scope {
            Scope::App => serde_json::to_value(crate::config::AppConfig::default())?,
            Scope::Reader => serde_json::to_value(crate::reader::defaults())?,
        });
    }
    let _lock = match scope {
        Scope::App => Some(crate::config::lock_config()?),
        Scope::Reader => None,
    };
    let mut current = match scope {
        Scope::App => serde_json::to_value(load_app_config()?)?,
        Scope::Reader => serde_json::to_value(crate::reader::load()?)?,
    };
    match command {
        ConfigCommand::Show => print(&current),
        ConfigCommand::Get { key } => print(
            current
                .pointer(&pointer(&key))
                .with_context(|| format!("unknown configuration key {key:?}; use config show"))?,
        ),
        ConfigCommand::Set { key, value } => {
            let next: Value = serde_json::from_str(&value)
                .context("value must be JSON; quote strings, e.g. '\"Space\"'")?;
            set_value(&mut current, &key, next)?;
            save_config(scope, current)
        }
        ConfigCommand::Import { path } => {
            let next: Value = serde_json::from_slice(&std::fs::read(path)?)?;
            save_config(scope, next)
        }
        ConfigCommand::Edit => {
            drop(_lock);
            let edit_value = match scope {
                Scope::App => serde_json::to_value(load_app_config()?)?,
                Scope::Reader => serde_json::to_value(crate::reader::load()?)?,
            };
            let edit_path = crate::config::unique_atomic_temp_path(&path);
            if let Some(parent) = edit_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(
                &edit_path,
                format!("{}\n", serde_json::to_string_pretty(&edit_value)?),
            )?;
            let result = (|| -> Result<()> {
                let editor = std::env::var("VISUAL")
                    .or_else(|_| std::env::var("EDITOR"))
                    .context("set VISUAL or EDITOR, or use config path to locate the JSON file")?;
                let mut words = editor.split_whitespace();
                let executable = words.next().context("EDITOR is empty")?;
                let status = std::process::Command::new(executable)
                    .args(words)
                    .arg(&edit_path)
                    .status()?;
                anyhow::ensure!(status.success(), "editor exited with {status}");
                let edited: Value = serde_json::from_slice(&std::fs::read(&edit_path)?)
                    .context("edited configuration is not valid JSON")?;
                let _lock = match scope {
                    Scope::App => Some(crate::config::lock_config()?),
                    Scope::Reader => None,
                };
                save_config(scope, edited)
            })();
            let _ = std::fs::remove_file(edit_path);
            result
        }
        ConfigCommand::Path | ConfigCommand::Defaults => unreachable!(),
    }
}

fn save_config(scope: Scope, value: Value) -> Result<()> {
    match scope {
        Scope::App => {
            let next: crate::config::AppConfig = serde_json::from_value(value)?;
            next.save()?;
            print(&json!({"saved": true, "path": crate::config::AppConfig::config_path()}))
        }
        Scope::Reader => {
            let next: reader_core::AppConfig = serde_json::from_value(value)?;
            let revision = next.revision;
            let next = crate::reader::store().save(next, revision)?;
            print(
                &json!({"saved": true, "path": crate::reader::store().path(), "revision": next.revision}),
            )
        }
    }
}

fn credentials(command: CredentialCommand) -> Result<()> {
    use reader_core::secrets;
    match command {
        CredentialCommand::Generate {
            kind: GeneratedCredentialKind::Server,
            replace,
            stdout,
        } => {
            if stdout {
                println!("{}", generated_server_token(None, true));
                return Ok(());
            }
            anyhow::ensure!(std::env::var_os("SIMPLE_TTS_SERVER_TOKEN").is_none(),
                "SIMPLE_TTS_SERVER_TOKEN overrides the saved token; unset it before generating a saved token");
            let existing = secrets::server_key()?;
            let token = generated_server_token(existing.as_deref(), replace);
            if existing.as_deref() != Some(token.as_str()) {
                secrets::set_server_key(&token)?;
            }
            println!("{token}");
            Ok(())
        }
        CredentialCommand::List => print(&json!({
            "ai": crate::cleanup::secrets::compatible_api_key()?.is_some(),
            "stt": crate::cleanup::secrets::stt_api_key()?.is_some(),
            "speech": secrets::speech_key()?.is_some(),
            "ocr": secrets::ocr_key()?.is_some(),
            "server": secrets::server_key()?.is_some(),
        })),
        CredentialCommand::Set { kind, stdin, env } => {
            anyhow::ensure!(
                stdin || env.is_some(),
                "choose a credential source: --stdin or --env NAME"
            );
            let secret = if let Some(name) = env {
                std::env::var(&name)
                    .with_context(|| format!("reading credential environment variable {name}"))?
            } else {
                anyhow::ensure!(
                    !io::stdin().is_terminal(),
                    "credential stdin is a terminal; pipe the value or use --env NAME"
                );
                let mut bytes = Vec::with_capacity(4096);
                io::stdin()
                    .take((MAX_SECRET_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                anyhow::ensure!(bytes.len() <= MAX_SECRET_BYTES, "credential exceeds 1 MiB");
                String::from_utf8(bytes)
                    .context("credential input is not valid UTF-8")?
                    .trim_end_matches(['\r', '\n'])
                    .to_owned()
            };
            anyhow::ensure!(!secret.trim().is_empty(), "credential cannot be empty");
            match kind {
                CredentialKind::Ai => crate::cleanup::secrets::set_compatible_api_key(&secret)?,
                CredentialKind::Stt => crate::cleanup::secrets::set_stt_api_key(&secret)?,
                CredentialKind::Speech => secrets::set_speech_key(&secret)?,
                CredentialKind::Ocr => secrets::set_ocr_key(&secret)?,
                CredentialKind::Server => secrets::set_server_key(&secret)?,
            }
            print(&json!({"saved": true, "kind": format!("{kind:?}").to_lowercase()}))
        }
        CredentialCommand::Remove { kind } => {
            match kind {
                RemovableCredentialKind::Ai => {
                    crate::cleanup::secrets::delete_compatible_api_key()?
                }
                RemovableCredentialKind::Stt => crate::cleanup::secrets::delete_stt_api_key()?,
                RemovableCredentialKind::Speech => secrets::delete_speech_key()?,
                RemovableCredentialKind::Ocr => secrets::delete_ocr_key()?,
            }
            print(&json!({"removed": true, "kind": format!("{kind:?}").to_lowercase()}))
        }
    }
}

fn generated_server_token(existing: Option<&str>, replace: bool) -> String {
    if !replace {
        if let Some(value) = existing.filter(|value| !value.trim().is_empty()) {
            return value.to_owned();
        }
    }
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn assets(command: AssetCommand) -> Result<()> {
    match command {
        AssetCommand::List => {
            let config = load_app_config()?;
            let root = crate::assets::root();
            let catalog: Vec<_> = crate::assets::catalog()
                .into_iter()
                .filter(crate::assets::available)
                .map(|asset| {
                    json!({
                        "id": asset.id,
                        "kind": asset.kind,
                        "platform": asset.platform,
                        "installed": crate::assets::installed_at(&root, &asset),
                        "directory": crate::assets::directory(&asset.id),
                        "dependencies": asset.dependencies,
                        "bytes": asset.bytes,
                        "asset": asset
                    })
                })
                .collect();
            let modules = crate::modules::inventory(&config);
            print(&json!({
                "assets": catalog,
                "models": modules["models"],
                "runtimes": modules["runtimes"],
                "config_paths": {
                    "app": crate::config::AppConfig::config_path(),
                    "reader": crate::reader::store().path()
                }
            }))
        }
        AssetCommand::Install { id } => {
            if id.ends_with(".gguf") {
                let cancellation = Arc::new(crate::models::DownloadCancellation::default());
                let signal = cancellation.clone();
                ctrlc::set_handler(move || {
                    signal.cancel();
                })?;
                let path = crate::models::download_model_cancellable(
                    &load_app_config()?,
                    &id,
                    &cancellation,
                    |_, _| {},
                )?;
                return print(&json!({"installed": id, "path": path}));
            }
            let manager = crate::assets::Manager::default();
            let stopped = Arc::new(AtomicBool::new(false));
            let signal = stopped.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::Release))?;
            manager.start(&id)?;
            let mut phase = String::new();
            loop {
                if stopped.load(Ordering::Acquire) {
                    manager.cancel(&id);
                }
                let progress = install_job(manager.jobs(), &id)?;
                if progress.phase != phase {
                    eprintln!("{}: {}", id, progress.phase);
                    phase.clone_from(&progress.phase);
                }
                match progress.phase.as_str() {
                    "ready" => {
                        return print(&json!({
                            "installed": id,
                            "directory": crate::assets::directory(&id),
                            "hint": "Use assets list for model/runtime IDs; assets select makes an explicit assignment."
                        }));
                    }
                    "failed" => bail!("{}", progress.error),
                    "cancelled" => bail!("download cancelled"),
                    _ => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        }
        AssetCommand::Remove { id } => {
            let stt = load_app_config()?;
            let reader = crate::reader::load()?;
            anyhow::ensure!(
                !is_assigned_or_dependency(&id, &stt, &reader),
                "asset is assigned; change its selection before removing it"
            );
            crate::assets::Manager::default().remove(&id)?;
            print(&json!({"removed": id}))
        }
        AssetCommand::Select {
            feature,
            model,
            runtime,
            language,
        } => select(feature, &model, &runtime, language.as_deref()),
    }
}

fn install_job(jobs: Vec<crate::assets::Progress>, id: &str) -> Result<crate::assets::Progress> {
    jobs.into_iter()
        .find(|job| job.job_id == id)
        .context("download job disappeared")
}

fn is_assigned_or_dependency(
    id: &str,
    stt: &crate::config::AppConfig,
    reader: &reader_core::AppConfig,
) -> bool {
    let catalog = crate::assets::catalog();
    let mut needed = std::collections::BTreeSet::new();
    for asset in &catalog {
        if crate::assets::assigned(&asset.id, stt, reader) {
            let mut pending = vec![asset.id.as_str()];
            while let Some(current) = pending.pop() {
                if !needed.insert(current.to_owned()) {
                    continue;
                }
                if let Some(current) = catalog.iter().find(|candidate| candidate.id == current) {
                    pending.extend(current.dependencies.iter().map(String::as_str));
                }
            }
        }
    }
    needed.contains(id)
}

fn select(feature: Feature, model: &str, runtime: &str, language: Option<&str>) -> Result<()> {
    use reader_core::routing::{self, SelectionMode};
    let registry = routing::registry();
    let feature_id = match feature {
        Feature::Stt => routing::Feature::Stt,
        Feature::Tts => routing::Feature::Tts,
        Feature::Ocr => routing::Feature::Ocr,
    };
    let runtime = registry
        .runtimes
        .iter()
        .find(|r| r.id == runtime && r.feature == feature_id)
        .context("unknown runtime ID for this feature; use assets list")?;
    if let Some(asset) = &runtime.asset {
        ensure_installed(asset)?;
    }
    if matches!(feature, Feature::Stt) {
        let model_file = crate::models::catalog_for_config(&load_app_config()?)
            .into_iter()
            .find(|item| item.file == model)
            .map(|item| item.file)
            .context("unknown STT model ID; use assets list")?;
        let _lock = crate::config::lock_config()?;
        let mut config = load_app_config()?;
        anyhow::ensure!(
            config.model_dir_path().join(&model_file).is_file(),
            "STT model file is not installed"
        );
        if let Some(language) = language {
            anyhow::ensure!(
                crate::config::valid_language_id(language),
                "invalid language ID"
            );
            config.speech.selection_mode = crate::config::ModelSelectionMode::FollowKeyboard;
            config
                .speech
                .language_models
                .insert(language.to_owned(), Some(model_file));
            config
                .speech
                .language_runtimes
                .insert(language.to_owned(), Some(runtime.id.clone()));
        } else {
            config.speech.selection_mode = crate::config::ModelSelectionMode::SingleModel;
            config.speech.single_model_filename = Some(model_file);
            if let Some(asset) = &runtime.asset {
                config.speech.runtime_dir = crate::assets::directory(asset)
                    .to_string_lossy()
                    .into_owned();
            }
        }
        config.save()?;
    } else {
        let model = registry
            .models
            .iter()
            .find(|m| m.id == model && m.feature == feature_id)
            .context("unknown model ID for this feature; use assets list")?;
        anyhow::ensure!(
            runtime.engine == model.engine,
            "model and runtime use different engines"
        );
        ensure_installed(&model.asset)?;
        let mut config = crate::reader::load()?;
        let selection = match feature {
            Feature::Tts => &mut config.speech.selection,
            Feature::Ocr => &mut config.ocr.selection,
            Feature::Stt => unreachable!(),
        };
        let binding = if let Some(language) = language {
            anyhow::ensure!(
                crate::config::valid_language_id(language),
                "invalid language ID"
            );
            selection.mode = SelectionMode::FollowKeyboard;
            selection
                .languages
                .entry(routing::primary(language))
                .or_default()
        } else {
            selection.mode = SelectionMode::SingleModel;
            &mut selection.single
        };
        *binding = routing::Binding {
            model: Some(model.id.clone()),
            runtime: Some(runtime.id.clone()),
            ..Default::default()
        };
        let revision = config.revision;
        crate::reader::store().save(config, revision)?;
    }
    print(&json!({"selected": true}))
}

fn ensure_installed(id: &str) -> Result<()> {
    anyhow::ensure!(
        crate::assets::catalog().iter().any(|asset| {
            asset.id == id
                && crate::assets::available(asset)
                && crate::assets::installed_at(&crate::assets::root(), asset)
        }),
        "asset {id} is not installed; run assets install {id}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_tracks_parent_job_while_a_dependency_is_downloading() {
        let progress = crate::assets::Progress {
            id: "paddle-det".into(),
            job_id: "paddle-ar-en".into(),
            phase: "downloading".into(),
            ..Default::default()
        };
        assert_eq!(
            install_job(vec![progress], "paddle-ar-en").unwrap().id,
            "paddle-det"
        );
    }

    #[test]
    fn generating_a_token_preserves_existing_clients_unless_rotation_requested() {
        assert_eq!(
            generated_server_token(Some("existing-token"), false),
            "existing-token"
        );
        let first = generated_server_token(None, false);
        let second = generated_server_token(Some(&first), true);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn keys_are_json_pointers_without_losing_escapes() {
        assert_eq!(pointer("server.enabled"), "/server/enabled");
        assert_eq!(pointer("a.b/c~d"), "/a/b~1c~0d");
        assert_eq!(
            pointer("/speech/language_models/en"),
            "/speech/language_models/en"
        );
    }

    #[test]
    fn set_supports_new_map_entries_and_json_pointer_escapes() {
        let mut value = json!({"speech":{"language_models":{}},"map":{"a/b~c":1}});
        set_value(&mut value, "speech.language_models.ar", json!("model")).unwrap();
        set_value(&mut value, "/map/a~1b~0c", json!(2)).unwrap();
        assert_eq!(value["speech"]["language_models"]["ar"], "model");
        assert_eq!(value["map"]["a/b~c"], 2);
    }

    #[test]
    fn set_rejects_invalid_paths_and_array_growth() {
        let mut value = json!({"a":[1]});
        assert!(set_value(&mut value, "a.2", json!(3)).is_err());
        assert!(set_value(&mut value, "/a/~2", json!(3)).is_err());
    }

    #[test]
    fn dependency_removal_is_blocked_for_assigned_assets() {
        let dependency = crate::assets::catalog()
            .into_iter()
            .find(|asset| !asset.dependencies.is_empty())
            .expect("catalog includes an asset with dependencies");
        let stt = crate::config::AppConfig::default();
        let mut reader = crate::reader::defaults();
        reader.speech.selection.single.model = Some(dependency.id.clone());
        let required = dependency.dependencies.first().unwrap();
        assert!(is_assigned_or_dependency(required, &stt, &reader));
        assert!(!is_assigned_or_dependency("unrelated-id", &stt, &reader));
    }
}
