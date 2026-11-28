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
    use test_case::test_case;

    #[test_case("localhost"; "localhost")]
    #[test_case("api.localhost"; "localhost subdomain")]
    #[test_case("service.local"; "mdns local")]
    #[test_case("127.0.0.1"; "ipv4 loopback")]
    #[test_case("10.0.1.2"; "ipv4 private 10")]
    #[test_case("192.168.1.1"; "ipv4 private 192 168")]
    #[test_case("172.16.0.5"; "ipv4 private 172 16")]
    #[test_case("169.254.169.254"; "ipv4 link local metadata")]
    #[test_case("0.0.0.0"; "ipv4 unspecified")]
    #[test_case("::1"; "ipv6 loopback")]
    #[test_case("::"; "ipv6 unspecified")]
    #[test_case("[::1]"; "bracketed ipv6 loopback")]
    #[test_case("[::ffff:127.0.0.1]"; "mapped ipv4 loopback")]
    #[test_case("[::ffff:10.0.0.1]"; "mapped ipv4 private 10")]
    #[test_case("[::ffff:192.168.1.1]"; "mapped ipv4 private 192 168")]
    #[test_case("fc00::1"; "ipv6 unique local fc")]
    #[test_case("fd12:3456:789a::1"; "ipv6 unique local fd")]
    #[test_case("fe80::1"; "ipv6 link local")]
    #[test_case("100.64.0.1"; "cgnat low")]
    #[test_case("100.127.255.254"; "cgnat high")]
    #[test_case("224.0.0.1"; "ipv4 multicast")]
    #[test_case("255.255.255.255"; "ipv4 broadcast")]
    #[test_case("240.0.0.1"; "ipv4 reserved")]
    #[test_case("192.0.0.8"; "ietf protocol assignments")]
    #[test_case("198.18.0.1"; "benchmarking low")]
    #[test_case("198.19.255.255"; "benchmarking high")]
    #[test_case("ff02::1"; "ipv6 multicast")]
    #[test_case("[64:ff9b::7f00:1]"; "nat64 loopback")]
    #[test_case("64:ff9b::a9fe:a9fe"; "nat64 link local")]
    #[test_case("64:ff9b::c0a8:101"; "nat64 private")]
    #[test_case("64:ff9b:1::1"; "nat64 local use prefix")]
    #[test_case("[2002:7f00:1::1]"; "6to4 loopback")]
    #[test_case("2002:a9fe:a9fe::"; "6to4 link local")]
    #[test_case("2002:0a00:0001::5"; "6to4 private")]
    #[test_case("2001:0:4136:e378:8000:63bf:3fff:fdd2"; "teredo")]
    fn classifies_private_hosts(host: &str) {
        assert!(is_private_host(host));
    }

    #[test_case("example.com"; "public domain")]
    #[test_case("93.184.216.34"; "public ipv4")]
    #[test_case("2606:4700:4700::1111"; "public ipv6")]
    #[test_case("100.128.0.1"; "just above cgnat")]
    #[test_case("198.17.255.255"; "just below benchmarking")]
    #[test_case("198.20.0.1"; "just above benchmarking")]
    #[test_case("192.0.1.1"; "just above ietf protocol assignments")]
    #[test_case("64:ff9b::5db8:d822"; "nat64 public")]
    #[test_case("2002:5db8:d822::1"; "6to4 public")]
    #[test_case("2001:4860:4860::8888"; "public 2001 ipv6")]
    fn classifies_public_hosts(host: &str) {
        assert!(!is_private_host(host));
    }

    #[test_case("https://example.com/api", false, true; "public https")]
    #[test_case("http://127.0.0.1:8080", false, false; "loopback blocked")]
    #[test_case("http://[::ffff:127.0.0.1]:8080", false, false; "mapped loopback blocked")]
    #[test_case("http://[fd00::1]:8080", false, false; "unique local blocked")]
    #[test_case("http://127.0.0.1:8080", true, true; "loopback allowed when private permitted")]
    #[test_case("file:///etc/passwd", false, false; "file scheme rejected")]
    #[test_case("http://user:pass@example.com/data", false, false; "credentials rejected")]
    #[test_case("http://admin@example.com/", false, false; "username rejected")]
    fn validates_url_security_rules(url: &str, allow: bool, ok: bool) {
        assert_eq!(validate_url(url, allow).is_ok(), ok);
    }
}
