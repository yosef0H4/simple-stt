use super::secrets;
use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPE: &str = "openid profile email offline_access";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatGptTokens {
    pub access: String,
    pub refresh: String,
    pub expires_at_ms: u64,
    pub account_id: Option<String>,
}

pub struct BrowserLogin {
    pub authorization_url: String,
    listener: TcpListener,
    verifier: String,
    state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceLogin {
    pub verification_url: String,
    pub user_code: String,
    device_auth_id: String,
    interval_secs: u64,
}

pub fn chatgpt_connected() -> bool {
    secrets::chatgpt_tokens().ok().flatten().is_some()
}

pub fn valid_chatgpt_tokens() -> Result<ChatGptTokens> {
    let raw = secrets::chatgpt_tokens()?.context("ChatGPT is not connected")?;
    let tokens: ChatGptTokens =
        serde_json::from_str(&raw).context("saved ChatGPT login is invalid")?;
    if tokens.expires_at_ms > now_ms().saturating_add(60_000) {
        return Ok(tokens);
    }
    refresh(&tokens.refresh)
}

pub fn begin_browser_login() -> Result<BrowserLogin> {
    let listener = TcpListener::bind("127.0.0.1:1455")
        .context("port 1455 is busy; close another Codex login and retry")?;
    listener.set_nonblocking(false)?;
    let verifier = random_urlsafe(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(32);
    let mut url = url::Url::parse(&format!("{ISSUER}/oauth/authorize"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "simple_stt");
    Ok(BrowserLogin {
        authorization_url: url.into(),
        listener,
        verifier,
        state,
    })
}

impl BrowserLogin {
    pub fn complete(self) -> Result<ChatGptTokens> {
        let (mut stream, _) = self
            .listener
            .accept()
            .context("waiting for ChatGPT login callback")?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut first_line = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut first_line)?;
        let path = first_line
            .split_whitespace()
            .nth(1)
            .context("invalid OAuth callback")?;
        let callback = url::Url::parse(&format!("http://localhost{path}"))?;
        let code = callback
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned());
        let state = callback
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned());
        let result = (|| {
            anyhow::ensure!(
                state.as_deref() == Some(self.state.as_str()),
                "ChatGPT login state did not match"
            );
            exchange_code(
                &code.context("ChatGPT login callback did not contain a code")?,
                &self.verifier,
                REDIRECT_URI,
            )
        })();
        let body = if result.is_ok() {
            "<!doctype html><title>Simple STT connected</title><p>ChatGPT is connected. You can close this tab.</p>"
        } else {
            "<!doctype html><title>Simple STT connection failed</title><p>ChatGPT could not be connected. Return to Simple STT Settings for details.</p>"
        };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes())?;
        result
    }
}

pub fn begin_device_login() -> Result<DeviceLogin> {
    let client = oauth_client()?;
    let response = client
        .post(format!("{ISSUER}/api/accounts/deviceauth/usercode"))
        .header(
            "User-Agent",
            concat!("simple-stt/", env!("CARGO_PKG_VERSION")),
        )
        .json(&json!({"client_id":CLIENT_ID}))
        .send()
        .context("starting ChatGPT device login")?;
    let status = response.status();
    let value: Value = response.json().context("decoding ChatGPT device login")?;
    anyhow::ensure!(
        status.is_success(),
        "ChatGPT device login returned HTTP {status}"
    );
    Ok(DeviceLogin {
        verification_url: format!("{ISSUER}/codex/device"),
        user_code: value
            .get("user_code")
            .and_then(Value::as_str)
            .context("device login did not return user_code")?
            .to_owned(),
        device_auth_id: value
            .get("device_auth_id")
            .and_then(Value::as_str)
            .context("device login did not return device_auth_id")?
            .to_owned(),
        interval_secs: value
            .get("interval")
            .and_then(Value::as_str)
            .and_then(|v| v.parse().ok())
            .unwrap_or(5)
            .max(1),
    })
}

impl DeviceLogin {
    pub fn complete(self) -> Result<ChatGptTokens> {
        let client = oauth_client()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10 * 60);
        while std::time::Instant::now() < deadline {
            let response = client
                .post(format!("{ISSUER}/api/accounts/deviceauth/token"))
                .header(
                    "User-Agent",
                    concat!("simple-stt/", env!("CARGO_PKG_VERSION")),
                )
                .json(&json!({"device_auth_id":self.device_auth_id,"user_code":self.user_code}))
                .send()
                .context("polling ChatGPT device login")?;
            if response.status().is_success() {
                let value: Value = response.json()?;
                let code = value
                    .get("authorization_code")
                    .and_then(Value::as_str)
                    .context("device login did not return authorization_code")?;
                let verifier = value
                    .get("code_verifier")
                    .and_then(Value::as_str)
                    .context("device login did not return code_verifier")?;
                return exchange_code(code, verifier, &format!("{ISSUER}/deviceauth/callback"));
            }
            anyhow::ensure!(
                matches!(response.status().as_u16(), 403 | 404),
                "ChatGPT device login failed with HTTP {}",
                response.status()
            );
            std::thread::sleep(Duration::from_secs(self.interval_secs + 1));
        }
        anyhow::bail!("ChatGPT device login timed out")
    }
}

fn exchange_code(code: &str, verifier: &str, redirect_uri: &str) -> Result<ChatGptTokens> {
    let client = oauth_client()?;
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .context("exchanging ChatGPT authorization code")?;
    token_response(response)
}

fn refresh(refresh_token: &str) -> Result<ChatGptTokens> {
    let client = oauth_client()?;
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .context("refreshing ChatGPT login")?;
    token_response(response)
}

fn token_response(response: reqwest::blocking::Response) -> Result<ChatGptTokens> {
    let status = response.status();
    let value: Value = response.json().context("decoding ChatGPT token response")?;
    anyhow::ensure!(
        status.is_success(),
        "ChatGPT token exchange returned HTTP {status}"
    );
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .context("token response did not contain access_token")?
        .to_owned();
    let refresh = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .context("token response did not contain refresh_token")?
        .to_owned();
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3_600);
    let account_id = extract_account_id(&access);
    let tokens = ChatGptTokens {
        access,
        refresh,
        expires_at_ms: now_ms().saturating_add(expires_in.saturating_mul(1_000)),
        account_id,
    };
    secrets::set_chatgpt_tokens(&serde_json::to_string(&tokens)?)?;
    Ok(tokens)
}

fn extract_account_id(access: &str) -> Option<String> {
    let payload = access.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
        .and_then(Value::as_str)
        .or_else(|| value.get("chatgpt_account_id").and_then(Value::as_str))
        .map(str::to_owned)
}

fn oauth_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .context("building OAuth client")
}

fn random_urlsafe(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_values_are_url_safe() {
        let value = random_urlsafe(48);
        assert!(!value.contains(['+', '/', '=']));
    }
}
