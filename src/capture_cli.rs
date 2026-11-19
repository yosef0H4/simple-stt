use crate::capture::state::ServiceState;
use crate::common::shell_protocol::{
    ClientMessage, ServerMessage, ShellCommand, ShellResponse, SHELL_PROTOCOL_VERSION,
};
use crate::config::AppConfig;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// File-only transcription client. Reuses the desktop service when available,
/// otherwise owns a disposable inference worker for the lifetime of this value.
#[derive(Default)]
pub struct Transcriber {
    worker: Option<crate::capture::inference_supervisor::WorkerSupervisor>,
}

impl Transcriber {
    /// Headless callers must run the same idle policy as the desktop service.
    pub fn shutdown_if_idle(&mut self) -> Result<()> {
        if let Some(worker) = self.worker.as_mut() {
            worker.shutdown_if_idle(false)?;
        }
        Ok(())
    }

    pub fn transcribe(&mut self, wav: &Path, language: Option<&str>) -> Result<String> {
        if service_is_running()? {
            // Release any private model before asking the desktop service to load one.
            if let Some(mut worker) = self.worker.take() {
                worker.shutdown_now()?;
            }
            return transcribe(wav, language);
        }
        let config = AppConfig::load()?;
        let normalized = normalized_language(language)?;
        let language = normalized.as_deref();
        let route_language = language.unwrap_or("single");
        if config.speech.provider_for(route_language)
            == crate::config::SpeechProvider::RemoteOpenAiCompatible
        {
            let remote_config = config.speech.remote_for(route_language);
            let remote = crate::remote_stt::transcribe_wav(&remote_config, wav, language);
            let fallback_enabled = remote_config.fallback_to_local;
            return crate::remote_stt::with_local_fallback(remote, fallback_enabled, || {
                let local_language = local_fallback_language(&config, language)?;
                let next = local_fallback_worker_config(&config, &local_language)?;
                let worker = match &mut self.worker {
                    Some(worker) => {
                        worker.replace_config(next)?;
                        worker
                    }
                    slot @ None => slot.insert(
                        crate::capture::inference_supervisor::WorkerSupervisor::new(next),
                    ),
                };
                worker.transcribe_wav(0, wav)
            });
        }
        let language = local_fallback_language(&config, language)?;
        let next = local_fallback_worker_config(&config, &language)?;
        let worker = match &mut self.worker {
            Some(worker) => {
                worker.replace_config(next)?;
                worker
            }
            slot @ None => slot.insert(
                crate::capture::inference_supervisor::WorkerSupervisor::new(next),
            ),
        };
        worker.transcribe_wav(0, wav)
    }
}

pub fn local_fallback_language(config: &AppConfig, requested: Option<&str>) -> Result<String> {
    if let Some(language) = normalized_language(requested)? {
        return Ok(language);
    }
    if config.speech.selection_mode == crate::config::ModelSelectionMode::SingleModel {
        return Ok("single".to_owned());
    }
    crate::capture::input_language::active_keyboard_language()
        .context("detecting keyboard language; specify --language to select a local fallback model")
}

pub fn local_fallback_worker_config(
    config: &AppConfig,
    language: &str,
) -> Result<crate::capture::inference_supervisor::WorkerConfig> {
    anyhow::ensure!(
        crate::config::valid_language_id(language),
        "invalid language identifier"
    );
    let model_path = config
        .model_path_for(language)
        .filter(|path| path.is_file())
        .with_context(|| {
            format!("no installed local STT model assigned for language {language}")
        })?;
    let runtime = crate::modules::stt_runtime(config, language)
        .filter(|path| crate::modules::runtime_ready(Some(path)))
        .with_context(|| format!("local STT runtime is not ready for language {language}"))?;
    if language != "single" {
        if let Some(filename) = model_path.file_name().and_then(|name| name.to_str()) {
            if let Some(model) = crate::models::catalog_for_config(config)
                .into_iter()
                .find(|model| model.file == filename)
            {
                let tag = format!("({language})");
                anyhow::ensure!(
                    model.languages.is_empty()
                        || model.languages.iter().any(|entry| entry.ends_with(&tag)),
                    "local STT model {filename} does not support language {language}"
                );
            }
        }
    }
    let current = std::env::current_exe().context("resolving inference executable")?;
    let executable = current
        .parent()
        .context("executable has no parent directory")?
        .join(format!("simple-stt-infer{}", std::env::consts::EXE_SUFFIX));
    Ok(crate::capture::inference_supervisor::WorkerConfig {
        executable,
        runtime_dir: runtime,
        model_path,
        log_path: AppConfig::infer_log_path(),
        log_level: config.diagnostics.log_level.clone(),
        inference_device: if config.speech.selection_mode
            == crate::config::ModelSelectionMode::FollowKeyboard
        {
            config
                .speech
                .language_devices
                .get(language)
                .copied()
                .unwrap_or(config.speech.inference_device)
        } else {
            config.speech.inference_device
        },
        speech_language: language.to_owned(),
        idle_timeout: Duration::from_secs(config.speech.idle_worker_timeout_secs),
        shutdown_grace: Duration::from_millis(config.speech.worker_shutdown_grace_ms),
    })
}

fn normalized_language(language: Option<&str>) -> Result<Option<String>> {
    let language = language.map(|value| match value.to_ascii_lowercase().as_str() {
        "english" => "en".to_owned(),
        "arabic" => "ar".to_owned(),
        _ => value.to_ascii_lowercase(),
    });
    if let Some(language) = language.as_deref() {
        anyhow::ensure!(
            crate::config::valid_language_id(language),
            "invalid language identifier"
        );
    }
    Ok(language)
}

// A failed authentication or command is not evidence that the service is absent.
fn service_is_running() -> Result<bool> {
    let path = AppConfig::service_state_path();
    if !path.exists() {
        return Ok(false);
    }
    let state = ServiceState::load(&path).context("reading capture service state")?;
    #[cfg(windows)]
    if capture_process_exited(state.pid) {
        return Ok(false);
    }
    let address: std::net::SocketAddr = state
        .address
        .parse()
        .context("invalid capture service address")?;
    match TcpStream::connect_timeout(&address, Duration::from_secs(1)) {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error).context("checking capture service availability"),
    }
}

#[cfg(windows)]
fn capture_process_exited(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        // Access denied is inconclusive. Only an invalid PID proves absence.
        return std::io::Error::last_os_error().raw_os_error()
            == Some(ERROR_INVALID_PARAMETER as i32);
    }
    let exited = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
    unsafe { CloseHandle(handle) };
    exited
}

/// Transcribe a prepared WAV through the running capture service and its warm worker.
pub fn transcribe(wav: &Path, language: Option<&str>) -> Result<String> {
    let state = ServiceState::load(&AppConfig::service_state_path())
        .context("capture service is not running; start Simple STT and retry")?;
    anyhow::ensure!(
        state.protocol == SHELL_PROTOCOL_VERSION,
        "capture service protocol is incompatible; restart Simple STT"
    );
    let token = state.authentication_token()?;
    let mut stream = TcpStream::connect(&state.address)
        .context("cannot connect to capture service; start or restart Simple STT")?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    write_message(
        &mut stream,
        &ClientMessage::Hello {
            protocol: SHELL_PROTOCOL_VERSION,
            token: token.trim().to_owned(),
        },
    )?;
    match read_message(&mut reader)? {
        ServerMessage::HelloAck { protocol, .. } if protocol == SHELL_PROTOCOL_VERSION => (),
        ServerMessage::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected capture handshake: {other:?}"),
    }
    let initial = request(
        &mut stream,
        &mut reader,
        1,
        ShellCommand::PollEvents { after_seq: 0 },
    )?;
    let mut after = initial
        .values
        .get("latest_seq")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let job_id = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64)
        ^ u64::from(std::process::id());
    let path = wav
        .to_str()
        .context("audio path is not valid Unicode")?
        .to_owned();
    request(
        &mut stream,
        &mut reader,
        2,
        ShellCommand::TranscribeFile {
            path,
            language: language.map(str::to_owned),
            job_id,
        },
    )?;
    let deadline = Instant::now() + Duration::from_secs(60 * 60);
    loop {
        let response = request(
            &mut stream,
            &mut reader,
            3,
            ShellCommand::PollEvents { after_seq: after },
        )?;
        for event in response.events {
            after = after.max(event.seq);
            if event.kind == "cli_transcription"
                && event
                    .values
                    .get("job_id")
                    .and_then(|s| s.parse::<u64>().ok())
                    == Some(job_id)
            {
                if let Some(error) = event.values.get("error") {
                    bail!("{error}");
                }
                return event
                    .values
                    .get("text")
                    .cloned()
                    .context("transcription response has no text");
            }
        }
        anyhow::ensure!(Instant::now() < deadline, "transcription timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn request(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    request_id: u64,
    command: ShellCommand,
) -> Result<ShellResponse> {
    write_message(
        stream,
        &ClientMessage::Command {
            request_id,
            command,
        },
    )?;
    match read_message(reader)? {
        ServerMessage::Response {
            request_id: actual,
            response,
        } if actual == request_id => {
            anyhow::ensure!(response.ok, "{}", response.message);
            Ok(response)
        }
        ServerMessage::Error { code, message } => bail!("{code}: {message}"),
        other => bail!("unexpected capture response: {other:?}"),
    }
}
fn write_message(stream: &mut TcpStream, message: &ClientMessage) -> Result<()> {
    serde_json::to_writer(&mut *stream, message)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
fn read_message(reader: &mut BufReader<TcpStream>) -> Result<ServerMessage> {
    let mut line = String::new();
    anyhow::ensure!(
        reader.read_line(&mut line)? > 0,
        "capture service disconnected"
    );
    anyhow::ensure!(line.len() <= 1024 * 1024, "capture response exceeds 1 MiB");
    Ok(serde_json::from_str(&line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_fallback_uses_requested_language_or_single_model_assignment() {
        let config = AppConfig::default();
        assert_eq!(
            local_fallback_language(&config, Some("Arabic")).unwrap(),
            "ar"
        );
        let mut config = config;
        config.speech.selection_mode = crate::config::ModelSelectionMode::SingleModel;
        assert_eq!(local_fallback_language(&config, None).unwrap(), "single");
    }
}
