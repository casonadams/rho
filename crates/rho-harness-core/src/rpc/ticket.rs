use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedTicket {
    pub endpoint_id: String,
    pub direct_addresses: Vec<String>,
    pub ws_port: Option<u16>,
    pub relay_url: Option<String>,
}

pub fn extract_ticket_b64(input: &str) -> &str {
    let mut raw = input.trim();
    raw = raw.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
    if let Some(pos) = raw.find("ticket=") {
        let after = &raw[pos + "ticket=".len()..];
        let end = after.find(['&', '#', ' ', '\'', '"', '>']).unwrap_or(after.len());
        raw = &after[..end];
    } else if let Some(end) = raw.find('&') {
        raw = &raw[..end];
    }
    raw.strip_prefix("rho_").unwrap_or(raw)
}

pub fn extract_session_id_from_url(input: &str) -> Option<String> {
    let raw = input.trim();
    if let Some(pos) = raw.find("session=") {
        let after = &raw[pos + "session=".len()..];
        let end = after.find(['&', '#', ' ', '\'', '"', '>']).unwrap_or(after.len());
        let val = &after[..end];
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}

pub fn parse_ticket_info(ticket_str: &str) -> Result<ParsedTicket, String> {
    let raw = extract_ticket_b64(ticket_str);
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|e| format!("invalid base64 ticket: {e}"))?;

    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("invalid ticket JSON: {e}"))?;

    let endpoint_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| json.get("node_id").and_then(|v| v.as_str()))
        .unwrap_or_default()
        .to_string();

    let mut direct_addresses = Vec::new();
    let mut relay_url = json.get("relay_url").and_then(|v| v.as_str()).map(ToString::to_string);
    if let Some(addrs) = json.get("addrs").and_then(|v| v.as_array()) {
        for a in addrs {
            if let Some(s) = a.as_str() {
                direct_addresses.push(s.to_string());
            } else if let Some(ip) = a.get("Ip").and_then(|v| v.as_str()) {
                direct_addresses.push(ip.to_string());
            } else if let Some(relay) = a.get("Relay").and_then(|v| v.as_str()) {
                relay_url = Some(relay.to_string());
            }
        }
    }
    let ws_port = json.get("ws_port").and_then(|v| v.as_u64()).map(|p| p as u16);

    Ok(ParsedTicket {
        endpoint_id,
        direct_addresses,
        ws_port,
        relay_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ticket_b64() {
        assert_eq!(extract_ticket_b64("rho_abc123"), "abc123");
        assert_eq!(extract_ticket_b64("abc123"), "abc123");
        assert_eq!(
            extract_ticket_b64("https://casonadams.github.io/rho/hub/#ticket=rho_abc123&session=sess-1"),
            "abc123"
        );
        assert_eq!(extract_ticket_b64("rho_abc123&session=sess-1"), "abc123");
        assert_eq!(
            extract_ticket_b64("<https://casonadams.github.io/rho/hub/#ticket=rho_abc123&session=sess-1>"),
            "abc123"
        );
    }

    #[test]
    fn test_extract_session_id_from_url() {
        assert_eq!(
            extract_session_id_from_url("https://example.com/#ticket=abc&session=sess-42"),
            Some("sess-42".to_string())
        );
        assert_eq!(extract_session_id_from_url("rho_abc"), None);
    }
}
