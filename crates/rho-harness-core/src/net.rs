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
    let [a, b, c, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b))
}

fn embedded_ipv4(s: [u16; 8], first: usize) -> std::net::Ipv4Addr {
    let [a, b] = s[first].to_be_bytes();
    let [c, d] = s[first + 1].to_be_bytes();
    std::net::Ipv4Addr::new(a, b, c, d)
}

pub fn is_private_ipv6(v6: std::net::Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
        return true;
    }
    let s = v6.segments();
    if (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if let Some(v4) = v6.to_ipv4_mapped().or_else(|| v6.to_ipv4()) {
        return is_private_ipv4(v4);
    }
    let nat64 = s[..6] == [0x64, 0xff9b, 0, 0, 0, 0];
    if nat64 {
        return is_private_ipv4(embedded_ipv4(s, 6));
    }
    // Local-use NAT64, 6to4 and Teredo embed an IPv4 address a translator or relay will connect to.
    if s[..3] == [0x64, 0xff9b, 1] {
        return true;
    }
    if s[0] == 0x2002 {
        return is_private_ipv4(embedded_ipv4(s, 1));
    }
    s[0] == 0x2001 && s[1] == 0
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
            "100.64.0.1",
            "100.127.255.254",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.0.8",
            "198.18.0.1",
            "198.19.255.255",
            "ff02::1",
            "[64:ff9b::7f00:1]",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b::c0a8:101",
            "64:ff9b:1::1",
            "[2002:7f00:1::1]",
            "2002:a9fe:a9fe::",
            "2002:0a00:0001::5",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        ];
        for host in private {
            assert!(is_private_host(host), "{host}");
        }
    }

    #[test]
    fn classifies_public_hosts() {
        for host in [
            "example.com",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "100.128.0.1",
            "198.17.255.255",
            "198.20.0.1",
            "192.0.1.1",
            "64:ff9b::5db8:d822",
            "2002:5db8:d822::1",
            "2001:4860:4860::8888",
        ] {
            assert!(!is_private_host(host), "{host}");
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
