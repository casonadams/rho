//! Deterministic network-scope validation: URL shape, scheme allowlist, and
//! private-network classification used by the host safety floor.

use crate::error::{AppError, Result};
use url::Url;

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0 Safari/537.36 rho/0.1.0";
pub const BRAVE_CHROME_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const LYNX_UA: &str = "Lynx/2.9.3 libwww-FM/2.14 SSL-MM/1.4.1 OpenSSL/4.0.0";

#[derive(Clone)]
pub struct HttpRequest<'a> {
    pub url: &'a str,
    pub user_agent: Option<&'a str>,
    pub timeout_sec: u64,
    pub max_bytes: usize,
}

pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let lower = host.to_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => is_private_ipv4(v4),
            std::net::IpAddr::V6(v6) => is_private_ipv6(v6),
        };
    }
    false
}

fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.octets()[0] == 0
}

fn is_private_ipv6(v6: std::net::Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() {
        return true;
    }
    if (v6.segments()[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    if (v6.segments()[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if let Some(v4) = v6.to_ipv4_mapped() {
        return is_private_ipv4(v4);
    }
    if let Some(v4) = v6.to_ipv4() {
        return is_private_ipv4(v4);
    }
    false
}

pub fn validate_url(raw_url: &str, allow_private_network: bool) -> Result<Url> {
    let parsed = Url::parse(raw_url).map_err(|e| AppError::Tool(format!("Invalid URL '{raw_url}': {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(AppError::Tool(format!("Unsupported URL scheme: '{other}'"))),
    }

    if !allow_private_network
        && let Some(host) = parsed.host_str()
        && is_private_host(host)
    {
        return Err(AppError::Tool(format!(
            "Access to private/local network host '{host}' is blocked for security"
        )));
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_private_and_public_hosts() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("api.localhost"));
        assert!(is_private_host("service.local"));
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("10.0.1.2"));
        assert!(is_private_host("192.168.1.1"));
        assert!(is_private_host("172.16.0.5"));
        assert!(is_private_host("169.254.169.254"));
        assert!(is_private_host("0.0.0.0"));

        assert!(is_private_host("::1"));
        assert!(is_private_host("::"));
        assert!(is_private_host("[::1]"));
        assert!(is_private_host("[::ffff:127.0.0.1]"));
        assert!(is_private_host("[::ffff:10.0.0.1]"));
        assert!(is_private_host("[::ffff:192.168.1.1]"));
        assert!(is_private_host("fc00::1"));
        assert!(is_private_host("fd12:3456:789a::1"));
        assert!(is_private_host("fe80::1"));

        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("93.184.216.34"));
        assert!(!is_private_host("2606:4700:4700::1111"));
    }

    #[test]
    fn validates_url_security_rules() {
        assert!(validate_url("https://example.com/api", false).is_ok());
        assert!(validate_url("http://127.0.0.1:8080", false).is_err());
        assert!(validate_url("http://[::ffff:127.0.0.1]:8080", false).is_err());
        assert!(validate_url("http://[fd00::1]:8080", false).is_err());
        assert!(validate_url("http://127.0.0.1:8080", true).is_ok());
        assert!(validate_url("file:///etc/passwd", false).is_err());
    }
}
