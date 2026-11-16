//! Bounded discovery for OpenAI-compatible provider model and voice catalogs.
use anyhow::{Context, Result};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const MAX_RESPONSE: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct DiscoveryRequest {
    #[serde(default)]
    pub key_slot: Option<String>,
    pub feature: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Discovery {
    pub models: Vec<Item>,
    pub voices: Vec<Item>,
    pub voices_supported: bool,
}

pub fn discover(request: &DiscoveryRequest, saved_key: Option<String>) -> Result<Discovery> {
    anyhow::ensure!(
        matches!(request.feature.as_str(), "stt" | "tts" | "ocr" | "cleanup"),
        "unsupported provider feature"
    );
    let base = validate_base(&request.base_url)?;
    let key = request
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
        .or(saved_key);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let models_url = endpoint(&base, "models")?;
    let models_response = send(&client, models_url, key.as_deref())?;
    let model_json = read_json(models_response)?;
    let models = list(&model_json)
        .into_iter()
        .filter(|model| supports_feature(model, &request.feature))
        .filter_map(item)
        .collect();
    let mut voices = Vec::new();
    let mut voices_supported = false;
    if request.feature == "tts" {
        if let Ok(url) = endpoint(&base, "audio/voices") {
            if let Ok(response) = send(&client, url, key.as_deref()) {
                if response.status().is_success() {
                    if let Ok(value) = read_json(response) {
                        voices = list(&value).into_iter().filter_map(item).collect();
                        voices_supported = !voices.is_empty();
                    }
                }
            }
        }
    }
    Ok(Discovery {
        models,
        voices,
        voices_supported,
    })
}

fn validate_base(value: &str) -> Result<url::Url> {
    reader_core::provider_url::discovery_base(value)
}
fn endpoint(base: &url::Url, suffix: &str) -> Result<url::Url> {
    let mut url = base.clone();
    let path = format!("{}/{}", url.path().trim_end_matches('/'), suffix);
    url.set_path(&path);
    Ok(url)
}
fn send(client: &Client, url: url::Url, key: Option<&str>) -> Result<Response> {
    let mut request = client.get(url);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .context("provider discovery request failed")?;
    match response.status().as_u16() {
        401 => anyhow::bail!("Provider authentication failed (HTTP 401). Check this feature's key and confirm it is activated on the server. Generating a key with --stdout alone does not activate it."),
        403 => anyhow::bail!("Provider access denied (HTTP 403). Check this key's permissions."),
        _ => anyhow::ensure!(response.status().is_success(), "Model discovery unavailable (HTTP {}); enter a model ID manually or check the URL.",response.status()),
    }
    if let Some(length) = response.content_length() {
        anyhow::ensure!(
            length <= MAX_RESPONSE,
            "provider discovery response is too large"
        );
    }
    Ok(response)
}
fn read_json(response: Response) -> Result<Value> {
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .context("reading provider discovery response")?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_RESPONSE,
        "provider discovery response is too large"
    );
    serde_json::from_slice(&bytes).context("provider discovery response is invalid JSON")
}
fn list(value: &Value) -> Vec<Value> {
    value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("voices").and_then(Value::as_array))
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default()
}
fn supports_feature(model: &Value, feature: &str) -> bool {
    model
        .get("capabilities")
        .and_then(Value::as_array)
        .is_none_or(|capabilities| {
            capabilities
                .iter()
                .any(|value| value.as_str() == Some(feature))
        })
}
fn item(value: Value) -> Option<Item> {
    let id = value
        .get("id")
        .or_else(|| value.get("name"))?
        .as_str()?
        .trim();
    if id.is_empty() {
        return None;
    }
    Some(Item {
        id: id.to_owned(),
        label: value
            .get("label")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_advertised_features_without_hiding_generic_provider_models() {
        assert!(supports_feature(
            &serde_json::json!({"id":"external-model"}),
            "stt"
        ));
        assert!(supports_feature(
            &serde_json::json!({"id":"configured-stt","capabilities":["stt"]}),
            "stt"
        ));
        assert!(!supports_feature(
            &serde_json::json!({"id":"kokoro-v1","capabilities":["tts"]}),
            "stt"
        ));
    }
    #[test]
    fn endpoint_preserves_prefix_and_rejects_url_secrets() {
        let base = validate_base("http://localhost:1234/v1/").unwrap();
        assert_eq!(endpoint(&base, "models").unwrap().path(), "/v1/models");
        assert!(validate_base("https://u:p@example.com/v1").is_err());
    }
    #[test]
    fn normalizes_catalog_shapes() {
        let value: Value =
            serde_json::from_str(r#"{"data":[{"id":"abc"},{"name":"xyz","label":"XYZ"}]}"#)
                .unwrap();
        let ids: Vec<_> = list(&value)
            .into_iter()
            .filter_map(item)
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, ["abc", "xyz"]);
    }

    #[test]
    fn local_provider_auth_and_voice_discovery() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (path, body) in [
                ("/v1/models", r#"{"data":[{"id":"m1"}]}"#),
                ("/v1/audio/voices", r#"{"data":[{"id":"v1","model":"m1"}]}"#),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]).to_ascii_lowercase();
                assert!(request.starts_with(&format!("get {path} ")));
                assert!(request.contains("authorization: bearer key"));
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let request = DiscoveryRequest {
            key_slot: None,
            feature: "tts".into(),
            base_url: format!("http://{address}/v1"),
            api_key: None,
            model: Some("m1".into()),
        };
        let result = discover(&request, Some("key".into())).unwrap();
        server.join().unwrap();
        assert_eq!(result.models[0].id, "m1");
        assert_eq!(result.voices[0].model.as_deref(), Some("m1"));
    }
}
