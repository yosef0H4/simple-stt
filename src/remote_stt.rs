//! OpenAI-compatible speech-to-text client.
//!
//! Credentials are read from the same secure store used by AI cleanup. Audio is
//! sent only to the configured transcription endpoint and is never written by
//! this module.

use crate::config::RemoteSttConfig;
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_AUDIO_BYTES: u64 = 512 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HARD_TIMEOUT: Duration = Duration::from_secs(300);
const RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(250), Duration::from_secs(1)];
const MAX_RETRIES: usize = RETRY_DELAYS.len();
static BOUNDARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// Transcribe a WAV file using OpenAI's `/v1/audio/transcriptions` multipart API.
pub fn transcribe_wav(
    config: &RemoteSttConfig,
    wav: &Path,
    language: Option<&str>,
) -> Result<String> {
    let metadata = std::fs::metadata(wav).with_context(|| format!("reading {}", wav.display()))?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= MAX_AUDIO_BYTES,
        "audio must be a regular nonempty WAV file no larger than 512 MiB"
    );
    let bytes = std::fs::read(wav).with_context(|| format!("reading {}", wav.display()))?;
    transcribe_bytes(config, &bytes, language)
}

/// Transcribe signed 16-bit mono 16 kHz recording samples without staging a file.
pub fn transcribe_pcm(
    config: &RemoteSttConfig,
    samples: &[i16],
    language: Option<&str>,
) -> Result<String> {
    let wav = pcm_to_wav(samples)?;
    transcribe_bytes(config, &wav, language)
}

fn pcm_to_wav(samples: &[i16]) -> Result<Vec<u8>> {
    let data_len = samples
        .len()
        .checked_mul(2)
        .context("recording is too large")?;
    anyhow::ensure!(
        data_len > 0 && data_len <= MAX_AUDIO_BYTES as usize - 44,
        "recording is empty or too large"
    );
    let mut wav = Vec::with_capacity(data_len + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((data_len + 36) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(wav)
}

fn transcribe_bytes(
    config: &RemoteSttConfig,
    audio: &[u8],
    language: Option<&str>,
) -> Result<String> {
    anyhow::ensure!(!audio.is_empty(), "audio is empty");
    anyhow::ensure!(
        !config.model.trim().is_empty() && config.model.len() <= 256,
        "remote STT model must contain 1 to 256 characters"
    );
    anyhow::ensure!(
        (1_000..=300_000).contains(&config.timeout_ms),
        "remote STT timeout_ms must be in [1000, 300000]"
    );
    let endpoint = endpoint(&config.base_url)?;
    let token = crate::cleanup::secrets::stt_api_key()?;
    transcribe_bytes_with_token(config, &endpoint, audio, language, token.as_deref())
}

fn transcribe_bytes_with_token(
    config: &RemoteSttConfig,
    endpoint: &url::Url,
    audio: &[u8],
    language: Option<&str>,
    token: Option<&str>,
) -> Result<String> {
    let hard_deadline = Instant::now() + HARD_TIMEOUT;
    let request_timeout = Duration::from_millis(config.timeout_ms);
    let mut last_error = None;
    for (attempt, retry_delay) in RETRY_DELAYS
        .iter()
        .copied()
        .chain(std::iter::once(Duration::ZERO))
        .enumerate()
    {
        let remaining = hard_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            last_error = Some("request exceeded the 300-second hard timeout".to_owned());
            break;
        }
        match transcribe_once(
            config,
            endpoint,
            audio,
            language,
            token,
            request_timeout.min(remaining),
            CONNECT_TIMEOUT.min(remaining),
        ) {
            Ok(text) => return Ok(text),
            Err(error) => {
                let retryable = error.retryable;
                last_error = Some(error.message);
                if !retryable || attempt == MAX_RETRIES {
                    break;
                }
                let delay =
                    retry_delay.min(hard_deadline.saturating_duration_since(Instant::now()));
                if delay.is_zero() {
                    break;
                }
                std::thread::sleep(delay);
            }
        }
    }
    anyhow::bail!(
        "remote STT failed; {}",
        last_error.unwrap_or_else(|| "request exceeded the 300-second hard timeout".to_owned())
    )
}

struct AttemptError {
    message: String,
    retryable: bool,
}

/// Return a successful remote result or, when enabled, run the local fallback.
/// If both paths fail, preserve both errors for the caller.
pub fn with_local_fallback(
    remote: Result<String>,
    fallback_to_local: bool,
    fallback: impl FnOnce() -> Result<String>,
) -> Result<String> {
    match remote {
        Ok(text) => Ok(text),
        Err(remote_error) if fallback_to_local => {
            tracing::warn!("remote STT failed; trying configured local fallback");
            fallback().map_err(|fallback_error| {
                anyhow::anyhow!(
                    "remote STT failed: {remote_error:#}; local STT fallback failed: {fallback_error:#}"
                )
            })
        }
        Err(remote_error) => Err(remote_error),
    }
}

fn transcribe_once(
    config: &RemoteSttConfig,
    endpoint: &url::Url,
    audio: &[u8],
    language: Option<&str>,
    token: Option<&str>,
    timeout: Duration,
    connect_timeout: Duration,
) -> std::result::Result<String, AttemptError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = BOUNDARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let boundary = format!("simple-stt-{}-{timestamp}-{sequence}", std::process::id());
    let body = multipart_body(&boundary, &config.model, audio, language).map_err(|error| {
        AttemptError {
            message: error.to_string(),
            retryable: false,
        }
    })?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .map_err(|error| AttemptError {
            message: format!("building HTTP client: {error}"),
            retryable: false,
        })?;
    let mut request = client
        .post(endpoint.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body);
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|error| AttemptError {
        message: format!("sending request: {error}"),
        retryable: error.is_connect() || error.is_timeout(),
    })?;
    let status = response.status();
    let body = response.text().map_err(|error| AttemptError {
        message: format!("reading response: {error}"),
        retryable: error.is_timeout() || error.is_connect(),
    })?;
    if !status.is_success() {
        return Err(AttemptError {
            message: format!("provider returned HTTP {status} (response body omitted)"),
            retryable: matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504),
        });
    }
    let response: TranscriptionResponse =
        serde_json::from_str(&body).map_err(|error| AttemptError {
            message: format!("response was not valid transcription JSON: {error}"),
            retryable: false,
        })?;
    let text = response.text.trim().to_owned();
    if text.is_empty() {
        return Err(AttemptError {
            message: "provider returned empty text".to_owned(),
            retryable: false,
        });
    }
    Ok(text)
}

fn endpoint(base_url: &str) -> Result<url::Url> {
    let mut base = reader_core::provider_url::parse(base_url)?;
    reader_core::provider_url::validate_stt_transport(&base)?;
    // Preserve the established STT shorthand for an origin without a path.
    if base.path().is_empty() || base.path() == "/" {
        base.set_path("/v1");
    }
    reader_core::provider_url::operation(base.as_str(), "audio/transcriptions")
}

fn multipart_body(
    boundary: &str,
    model: &str,
    audio: &[u8],
    language: Option<&str>,
) -> Result<Vec<u8>> {
    let mut body = Vec::with_capacity(audio.len() + 512);
    field(&mut body, boundary, "model", model.as_bytes());
    if let Some(language) = language.filter(|value| !value.is_empty() && *value != "single") {
        anyhow::ensure!(
            language.len() <= 80 && !language.contains(['\r', '\n']),
            "invalid remote STT language"
        );
        field(&mut body, boundary, "language", language.as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(base_url: String) -> RemoteSttConfig {
        RemoteSttConfig {
            base_url,
            model: "whisper-test".to_owned(),
            fallback_to_local: true,
            timeout_ms: 10_000,
        }
    }

    #[test]
    fn multipart_transcription_uses_bearer_and_standard_fields() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}/v1", server.server_addr());
        let handle = std::thread::spawn(move || {
            let mut request = server.recv().unwrap();
            assert_eq!(request.method(), &tiny_http::Method::Post);
            assert_eq!(request.url(), "/v1/audio/transcriptions");
            assert_eq!(
                request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("Authorization"))
                    .unwrap()
                    .value
                    .as_str(),
                "Bearer remote-test-token"
            );
            let content_type = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Content-Type"))
                .unwrap()
                .value
                .as_str()
                .to_owned();
            assert!(content_type.starts_with("multipart/form-data; boundary="));
            let mut body = Vec::new();
            request.as_reader().read_to_end(&mut body).unwrap();
            let body = String::from_utf8_lossy(&body);
            assert!(body.contains("name=\"model\"\r\n\r\nwhisper-test"));
            assert!(body.contains("name=\"language\"\r\n\r\nen"));
            assert!(body.contains("name=\"file\"; filename=\"audio.wav\""));
            assert!(body.contains("Content-Type: audio/wav"));
            request
                .respond(tiny_http::Response::from_string(
                    r#"{"text":" Hello 世界 "}"#,
                ))
                .unwrap();
        });
        let endpoint = endpoint(&address).unwrap();
        let text = transcribe_bytes_with_token(
            &config(address),
            &endpoint,
            b"RIFFtest",
            Some("en"),
            Some("remote-test-token"),
        )
        .unwrap();
        assert_eq!(text, "Hello 世界");
        handle.join().unwrap();
    }

    #[test]
    fn endpoint_appends_standard_path_to_origin_or_v1_prefix() {
        assert_eq!(
            endpoint("https://api.example.test").unwrap().as_str(),
            "https://api.example.test/v1/audio/transcriptions"
        );
        assert_eq!(
            endpoint("https://api.example.test/v1/").unwrap().as_str(),
            "https://api.example.test/v1/audio/transcriptions"
        );
        assert_eq!(
            endpoint("https://api.example.test/v1/audio/transcriptions")
                .unwrap()
                .as_str(),
            "https://api.example.test/v1/audio/transcriptions"
        );
        assert!(endpoint("http://api.example.test/v1").is_err());
        assert!(endpoint("https://user:pass@api.example.test/v1").is_err());
    }

    #[test]
    fn private_server_and_custom_prefix_are_usable() {
        assert_eq!(
            endpoint("http://100.85.156.101:8765/v1").unwrap().path(),
            "/v1/audio/transcriptions"
        );
        assert_eq!(
            endpoint("https://host.test/proxy/api").unwrap().path(),
            "/proxy/api/audio/transcriptions"
        );
    }

    #[test]
    fn request_budgets_allow_cold_start_and_bound_connection_time() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(3));
        assert_eq!(HARD_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn transient_provider_failure_is_retried_within_the_attempt_bound() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}/v1", server.server_addr());
        let handle = std::thread::spawn(move || {
            let first = server.recv().unwrap();
            first
                .respond(
                    tiny_http::Response::from_string("temporarily unavailable")
                        .with_status_code(503),
                )
                .unwrap();
            let second = server.recv().unwrap();
            second
                .respond(tiny_http::Response::from_string(r#"{"text":"retried"}"#))
                .unwrap();
        });
        let endpoint = endpoint(&address).unwrap();
        let text =
            transcribe_bytes_with_token(&config(address), &endpoint, b"RIFFtest", None, None)
                .unwrap();
        assert_eq!(text, "retried");
        handle.join().unwrap();
    }

    #[test]
    fn persistent_transient_failure_stops_after_two_retries() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}/v1", server.server_addr());
        let handle = std::thread::spawn(move || {
            for _ in 0..=MAX_RETRIES {
                let request = server.recv().unwrap();
                request
                    .respond(
                        tiny_http::Response::from_string("temporarily unavailable")
                            .with_status_code(503),
                    )
                    .unwrap();
            }
        });
        let endpoint = endpoint(&address).unwrap();
        let error =
            transcribe_bytes_with_token(&config(address), &endpoint, b"RIFFtest", None, None)
                .unwrap_err();
        assert!(error.to_string().contains("HTTP 503"));
        handle.join().unwrap();
    }

    #[test]
    fn local_fallback_setting_controls_fallback_invocation() {
        let mut called = false;
        let result = with_local_fallback(Err(anyhow::anyhow!("remote timeout")), true, || {
            called = true;
            Ok("local transcript".to_owned())
        })
        .unwrap();
        assert!(called);
        assert_eq!(result, "local transcript");

        let result = with_local_fallback(Err(anyhow::anyhow!("remote timeout")), false, || {
            panic!("disabled fallback must not run")
        });
        assert_eq!(result.unwrap_err().to_string(), "remote timeout");

        let error = with_local_fallback(Err(anyhow::anyhow!("remote unavailable")), true, || {
            Err(anyhow::anyhow!("local model missing"))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("remote unavailable"));
        assert!(error.contains("local model missing"));
    }

    #[test]
    fn pcm_is_encoded_as_mono_16khz_wav() {
        let bytes = pcm_to_wav(&[1, -1]).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            16_000
        );
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
        assert_eq!(&bytes[44..], &[1, 0, 255, 255]);
    }
}
