//! URL handling shared by provider validation, discovery, and inference.
use anyhow::{bail, Context, Result};
use url::Url;

const OPERATIONS: &[&str] = &[
    "audio/transcriptions",
    "audio/speech",
    "audio/voices",
    "chat/completions",
    "responses",
    "models",
];

pub fn parse(value: &str) -> Result<Url> {
    let url = Url::parse(value.trim()).context("provider URL must be absolute")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "provider URL must use HTTP or HTTPS and include a host"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "provider URL must not contain credentials or a fragment"
    );
    Ok(url)
}

pub fn operation(value: &str, operation: &str) -> Result<Url> {
    let mut url = parse(value)?;
    let path = url.path().trim_end_matches('/').to_owned();
    if path.ends_with(&format!("/{operation}")) {
        url.set_path(&path);
        return Ok(url);
    }
    for suffix in OPERATIONS {
        if path.ends_with(&format!("/{suffix}")) {
            bail!("provider URL ends with /{suffix}; use an API base URL or the /{operation} endpoint");
        }
    }
    url.set_path(&format!("{path}/{operation}"));
    Ok(url)
}

pub fn discovery_base(value: &str) -> Result<Url> {
    let mut url = parse(value)?;
    let path = url.path().trim_end_matches('/');
    let base = OPERATIONS
        .iter()
        .find_map(|suffix| path.strip_suffix(&format!("/{suffix}")))
        .unwrap_or(path)
        .to_owned();
    url.set_path(&base);
    Ok(url)
}

/// STT keeps its HTTPS requirement outside loopback and private IP networks.
pub fn validate_stt_transport(url: &Url) -> Result<()> {
    let private = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        _ => false,
    };
    anyhow::ensure!(
        url.scheme() == "https" || private,
        "remote STT requires HTTPS outside loopback and private IP networks"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_and_full_endpoints_preserve_prefix_and_query() {
        for suffix in [
            "audio/speech",
            "audio/transcriptions",
            "responses",
            "chat/completions",
        ] {
            let expected = format!("https://host/proxy/API/{suffix}?api-version=2026-01-01");
            assert_eq!(
                operation("https://host/proxy/API/?api-version=2026-01-01", suffix)
                    .unwrap()
                    .as_str(),
                expected
            );
            assert_eq!(operation(&expected, suffix).unwrap().as_str(), expected);
            assert_eq!(
                discovery_base(&expected).unwrap().as_str(),
                "https://host/proxy/API?api-version=2026-01-01"
            );
        }
        assert!(operation("https://host/v1/audio/speech", "audio/transcriptions").is_err());
        assert!(parse("https://u:p@host/v1").is_err());
        assert!(parse("https://host/v1#key").is_err());
    }

    #[test]
    fn stt_accepts_private_networks_and_keeps_public_https_requirement() {
        for host in [
            "100.85.156.101",
            "192.168.50.208",
            "10.0.0.1",
            "127.0.0.1",
            "[::1]",
        ] {
            validate_stt_transport(&parse(&format!("http://{host}:8765/v1")).unwrap()).unwrap();
        }
        assert!(validate_stt_transport(&parse("http://public.example/v1").unwrap()).is_err());
    }
}
