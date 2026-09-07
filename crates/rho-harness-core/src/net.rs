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
    pub pdf_max_bytes: Option<usize>,
}

pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_ipv4(v4),
        std::net::IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let lower = host.to_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    false
}

pub fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.octets()[0] == 0
}

pub fn is_private_ipv6(v6: std::net::Ipv6Addr) -> bool {
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

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::Tool(
            "URLs containing credentials (username or password) are blocked for security".to_string(),
        ));
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
    fn classifies_private_hosts() {
        let private = [
            "localhost",
            "api.localhost",
            "service.local",
            "127.0.0.1",
            "10.0.1.2",
            "192.168.1.1",
            "172.16.0.5",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "::",
            "[::1]",
            "[::ffff:127.0.0.1]",
            "[::ffff:10.0.0.1]",
            "[::ffff:192.168.1.1]",
            "fc00::1",
            "fd12:3456:789a::1",
            "fe80::1",
        ];
        for host in private {
            assert!(is_private_host(host));
        }
    }

    #[test]
    fn classifies_public_hosts() {
        for host in ["example.com", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(!is_private_host(host));
        }
    }

    #[test]
    fn validates_url_security_rules() {
        let cases = [
            ("https://example.com/api", false, true),
            ("http://127.0.0.1:8080", false, false),
            ("http://[::ffff:127.0.0.1]:8080", false, false),
            ("http://[fd00::1]:8080", false, false),
            ("http://127.0.0.1:8080", true, true),
            ("file:///etc/passwd", false, false),
            ("http://user:pass@example.com/data", false, false),
            ("http://admin@example.com/", false, false),
        ];
        for (url, allow, ok) in cases {
            assert_eq!(validate_url(url, allow).is_ok(), ok);
        }
    }
}
