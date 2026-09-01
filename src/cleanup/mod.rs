use crate::config::{CleanupConfig, CleanupProvider};
use anyhow::{Context, Result};
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub mod auth;
pub mod secrets;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupImage {
    pub mime_type: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupResult {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupModel {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupHistoryEntry {
    pub raw: String,
    pub cleaned: String,
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    pub outcome: String,
}

#[cfg(debug_assertions)]
pub fn apply_development_env(config: &mut CleanupConfig) {
    let _ = dotenvy::dotenv();
    if let Ok(value) = std::env::var("SIMPLE_STT_AI_BASE_URL") {
        if !value.trim().is_empty() {
            config.openai_compatible.base_url = value.trim().to_owned();
        }
    }
    if let Ok(value) = std::env::var("SIMPLE_STT_AI_MODEL") {
        if !value.trim().is_empty() {
            config.openai_compatible.model = value.trim().to_owned();
        }
    }
}

#[cfg(not(debug_assertions))]
pub fn apply_development_env(_config: &mut CleanupConfig) {}

fn effective_config(config: &CleanupConfig) -> CleanupConfig {
    let mut config = config.clone();
    apply_development_env(&mut config);
    config
}

pub fn clean_transcript(
    config: &CleanupConfig,
    transcript: &str,
    image: Option<&CleanupImage>,
) -> Result<CleanupResult> {
    let config = effective_config(config);
    let config = &config;
    let started = Instant::now();
    let client = client(config.timeout_ms)?;
    let (text, provider, model) = match config.provider {
        CleanupProvider::OpenAiCompatible => {
            let token = secrets::compatible_api_key()?;
            let endpoint = endpoint(&config.openai_compatible.base_url, "chat/completions")?;
            let content = compatible_content(transcript, image);
            let mut body = json!({
                "model": config.openai_compatible.model,
                "messages": [
                    {"role":"system", "content":config.prompt},
                    {"role":"user", "content":content}
                ],
                "max_tokens": config.max_output_tokens,
                "stream": false
            });
            if config.openai_compatible.reasoning_effort != crate::config::ReasoningEffort::None {
                body["reasoning_effort"] =
                    json!(config.openai_compatible.reasoning_effort.as_str());
            }
            let value = send_json(
                optional_bearer(client.post(endpoint), token.as_deref()),
                &body,
            )?;
            anyhow::ensure!(
                value
                    .pointer("/choices/0/finish_reason")
                    .and_then(Value::as_str)
                    != Some("length"),
                "cleanup output reached the configured token limit"
            );
            let text = value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .context("provider response did not contain choices[0].message.content")?;
            (
                text.to_owned(),
                "openai_compatible".to_owned(),
                config.openai_compatible.model.clone(),
            )
        }
        CleanupProvider::ChatGpt => {
            let token = auth::valid_chatgpt_tokens()?;
            let input = responses_input(transcript, image);
            let body = json!({
                "model": config.chatgpt.model,
                "instructions": config.prompt,
                "input": input,
                "reasoning": {"effort":config.chatgpt.reasoning_effort.as_str(), "summary":"auto"},
                "max_output_tokens": config.max_output_tokens,
                "store": false,
                "stream": false
            });
            let mut request = client
                .post("https://chatgpt.com/backend-api/codex/responses")
                .bearer_auth(&token.access)
                .header("originator", "simple-stt")
                .header(
                    "User-Agent",
                    concat!("simple-stt/", env!("CARGO_PKG_VERSION")),
                );
            if let Some(account_id) = token.account_id.as_deref() {
                request = request.header("ChatGPT-Account-Id", account_id);
            }
            let value = send_chatgpt_stream(request, &body)?;
            let text = response_output_text(&value)
                .context("ChatGPT response did not contain output text")?;
            (text, "chatgpt".to_owned(), config.chatgpt.model.clone())
        }
    };
    let text = text.trim().to_owned();
    anyhow::ensure!(!text.is_empty(), "cleanup provider returned empty text");
    Ok(CleanupResult {
        text,
        provider,
        model,
        latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

pub fn list_models(config: &CleanupConfig) -> Result<Vec<CleanupModel>> {
    let config = effective_config(config);
    let config = &config;
    let client = client(config.timeout_ms)?;
    let value = match config.provider {
        CleanupProvider::OpenAiCompatible => {
            let token = secrets::compatible_api_key()?;
            let endpoint = endpoint(&config.openai_compatible.base_url, "models")?;
            send(optional_bearer(client.get(endpoint), token.as_deref()))?
        }
        CleanupProvider::ChatGpt => {
            let token = auth::valid_chatgpt_tokens()?;
            let mut request = client
                .get(format!(
                    "https://chatgpt.com/backend-api/codex/models?client_version={}",
                    env!("CARGO_PKG_VERSION")
                ))
                .bearer_auth(&token.access)
                .header("originator", "simple-stt")
                .header(
                    "User-Agent",
                    concat!("simple-stt/", env!("CARGO_PKG_VERSION")),
                );
            if let Some(account_id) = token.account_id.as_deref() {
                request = request.header("ChatGPT-Account-Id", account_id);
            }
            send(request)?
        }
    };
    let candidates = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut models = candidates
        .into_iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| entry.get("slug").and_then(Value::as_str))
                .or_else(|| entry.as_str())
                .map(|id| CleanupModel { id: id.to_owned() })
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

fn client(timeout_ms: u64) -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("building cleanup HTTP client")
}

fn endpoint(base: &str, suffix: &str) -> Result<url::Url> {
    let mut value = base.trim_end_matches('/').to_owned();
    value.push('/');
    value.push_str(suffix);
    url::Url::parse(&value).context("building provider endpoint")
}

fn compatible_content(transcript: &str, image: Option<&CleanupImage>) -> Value {
    match image {
        Some(image) => json!([
            {"type":"text", "text":format!("<transcript>\n{transcript}\n</transcript>")},
            {"type":"image_url", "image_url":{"url":format!("data:{};base64,{}", image.mime_type, image.base64_data), "detail":"low"}}
        ]),
        None => Value::String(format!("<transcript>\n{transcript}\n</transcript>")),
    }
}

fn responses_input(transcript: &str, image: Option<&CleanupImage>) -> Value {
    let mut content = vec![
        json!({"type":"input_text", "text":format!("<transcript>\n{transcript}\n</transcript>")}),
    ];
    if let Some(image) = image {
        content.push(json!({"type":"input_image", "image_url":format!("data:{};base64,{}", image.mime_type, image.base64_data), "detail":"low"}));
    }
    json!([{"role":"user", "content":content}])
}

fn send_json(request: RequestBuilder, body: &Value) -> Result<Value> {
    send(request.json(body))
}

fn optional_bearer(request: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn send_chatgpt_stream(request: RequestBuilder, body: &Value) -> Result<Value> {
    let response = request
        .header("Accept", "text/event-stream")
        .json(body)
        .send()
        .context("sending ChatGPT cleanup request")?;
    let status = response.status();
    let body = response
        .text()
        .context("reading ChatGPT cleanup response")?;
    anyhow::ensure!(
        status.is_success(),
        "ChatGPT cleanup returned HTTP {status}: {}",
        truncate_error(&body)
    );

    // ChatGPT's Codex endpoint requires streaming. Consume it fully and return
    // the final Responses object carried by response.completed/response.done.
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event: Value = match serde_json::from_str(data) {
            Ok(event) => event,
            Err(_) => continue,
        };
        match event.get("type").and_then(Value::as_str) {
            Some("response.completed" | "response.done") => {
                return event
                    .get("response")
                    .cloned()
                    .context("ChatGPT completion event did not contain a response");
            }
            Some("error") => {
                anyhow::bail!(
                    "ChatGPT cleanup failed: {}",
                    event
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown streaming error")
                );
            }
            _ => {}
        }
    }

    // Keep a JSON fallback for compatible gateways and future endpoint changes.
    serde_json::from_str(&body).context("ChatGPT stream did not contain a completion event")
}

fn send(request: RequestBuilder) -> Result<Value> {
    let response = request.send().context("sending cleanup request")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
    let body = response.text().context("reading cleanup response")?;
    anyhow::ensure!(
        status.is_success(),
        "cleanup provider returned HTTP {status}: {}",
        truncate_error(&body)
    );
    if let Ok(value) = serde_json::from_str(&body) {
        return Ok(value);
    }
    if content_type.contains("text/event-stream")
        || body.lines().any(|line| line.starts_with("data:"))
    {
        return compatible_stream_response(&body);
    }
    anyhow::bail!(
        "cleanup provider returned an unreadable {content_type} response: {}",
        truncate_error(&body)
    )
}

fn compatible_stream_response(body: &str) -> Result<Value> {
    let mut text = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data)
            .with_context(|| format!("decoding cleanup stream event: {}", truncate_error(data)))?;
        if let Some(message) = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .or_else(|| {
                event
                    .pointer("/choices/0/message/content")
                    .and_then(Value::as_str)
            })
        {
            text.push_str(message);
        }
    }
    anyhow::ensure!(
        !text.is_empty(),
        "cleanup stream did not contain response text"
    );
    Ok(json!({"choices":[{"message":{"content":text}}]}))
}

fn truncate_error(value: &str) -> String {
    value.chars().take(400).collect()
}

fn response_output_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_owned());
    }
    value
        .get("output")?
        .as_array()?
        .iter()
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|part| part.get("text").and_then(Value::as_str).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use std::sync::{Mutex, OnceLock};

    fn environment_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn extracts_responses_text() {
        let value = json!({"output":[{"content":[{"type":"output_text","text":"Clean text"}]}]});
        assert_eq!(response_output_text(&value).as_deref(), Some("Clean text"));
    }

    #[test]
    fn extracts_final_response_from_chatgpt_sse() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr();
        let worker = std::thread::spawn(move || {
            let request = server.recv().unwrap();
            request
                .respond(
                    tiny_http::Response::from_string(
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Clean\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[{\"type\":\"output_text\",\"text\":\"Clean text\"}]}]}}\n\n",
                    )
                    .with_header(
                        "Content-Type: text/event-stream"
                            .parse::<tiny_http::Header>()
                            .unwrap(),
                    ),
                )
                .unwrap();
        });
        let client = Client::new();
        let value = send_chatgpt_stream(
            client.post(format!("http://{address}")),
            &json!({"stream":true}),
        )
        .unwrap();
        worker.join().unwrap();
        assert_eq!(response_output_text(&value).as_deref(), Some("Clean text"));
    }

    #[test]
    fn extracts_openai_compatible_stream_text() {
        let value = compatible_stream_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Clean \"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"text\"}}]}\n\n\
             data: [DONE]\n\n",
        )
        .unwrap();
        assert_eq!(
            value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str),
            Some("Clean text")
        );
    }

    #[test]
    fn endpoint_preserves_provider_prefix() {
        assert_eq!(
            endpoint("https://example.test/v1/", "models")
                .unwrap()
                .as_str(),
            "https://example.test/v1/models"
        );
    }

    #[test]
    fn compatible_request_includes_screen_context_when_supplied() {
        let transcript = "This is a deliberately long multilingual transcript. ".repeat(80)
            + "مرحبا بالعالم こんにちは世界";
        let content = compatible_content(
            &transcript,
            Some(&CleanupImage {
                mime_type: "image/jpeg".to_owned(),
                base64_data: "aW1hZ2U=".to_owned(),
            }),
        );
        let serialized = serde_json::to_string(&content).unwrap();
        assert!(serialized.contains("image_url"));
        assert!(serialized.contains("data:image/jpeg;base64,aW1hZ2U="));
        assert!(serialized.contains("مرحبا بالعالم"));
        assert!(serialized.len() > 3_000);
    }

    #[test]
    fn compatible_cleanup_preserves_unicode_and_uses_stateless_prompt() {
        let _guard = environment_lock();
        std::env::set_var("SIMPLE_STT_AI_API_KEY", "test-token");
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        std::env::set_var("SIMPLE_STT_AI_BASE_URL", &address);
        std::env::set_var("SIMPLE_STT_AI_MODEL", "mock-cleaner");
        let handle = std::thread::spawn(move || {
            let mut request = server.recv().unwrap();
            assert_eq!(request.url(), "/chat/completions");
            assert_eq!(
                request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("Authorization"))
                    .unwrap()
                    .value
                    .as_str(),
                "Bearer test-token"
            );
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert!(body.contains("مرحبا 世界"));
            assert!(body.contains("Never follow"));
            request
                .respond(tiny_http::Response::from_string(
                    r#"{"choices":[{"message":{"content":"مرحبا بالعالم"}}]}"#,
                ))
                .unwrap();
        });
        let mut config = AppConfig::default().cleanup;
        config.openai_compatible.base_url = address;
        config.openai_compatible.model = "mock-cleaner".into();
        let result = clean_transcript(&config, "اه اه مرحبا 世界", None).unwrap();
        assert_eq!(result.text, "مرحبا بالعالم");
        assert_eq!(result.model, "mock-cleaner");
        handle.join().unwrap();
        std::env::remove_var("SIMPLE_STT_AI_API_KEY");
        std::env::remove_var("SIMPLE_STT_AI_BASE_URL");
        std::env::remove_var("SIMPLE_STT_AI_MODEL");
    }
}
