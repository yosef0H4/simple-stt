//! Bounded diagnostics from structured provider errors; never display raw bodies.
pub fn http_error(provider: &str, status: &str, body: &[u8], secret: &str) -> String {
    let prefix = format!("{provider} returned HTTP {status}");
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return prefix;
    };
    let Some(detail) = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(|value| value.as_str())
    else {
        return prefix;
    };
    let detail = if secret.is_empty() {
        detail.to_owned()
    } else {
        detail.replace(secret, "[redacted]")
    };
    static DATA: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"data:[^\s,]+,[A-Za-z0-9+/=_-]+").unwrap());
    let detail = DATA.replace_all(&detail, "[image data]");
    let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail: String = detail
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    if detail.is_empty() {
        prefix
    } else {
        format!("{prefix}: {detail}")
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn structured_rejection_keeps_the_reason_and_redacts_credentials_and_images() {
        let body = serde_json::json!({"error":{"message":"Unsupported voice alloy; key=test-secret; image=data:image/png;base64,YWJjZA=="}});
        let text = super::http_error(
            "speech provider",
            "400 Bad Request",
            &serde_json::to_vec(&body).unwrap(),
            "test-secret",
        );
        assert!(text.contains("Unsupported voice alloy"));
        assert!(!text.contains("test-secret") && !text.contains("YWJjZA=="));
        assert!(
            !super::http_error("vision provider", "400", b"<html>private data</html>", "")
                .contains("private")
        );
    }
}
