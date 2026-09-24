use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[derive(Serialize, Deserialize)]
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

#[wasm_bindgen]
pub fn parse_ticket(ticket_str: &str) -> Result<JsValue, JsValue> {
    let raw = extract_ticket_b64(ticket_str);
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|e| JsValue::from_str(&format!("invalid base64 ticket: {e}")))?;

    let json: Value =
        serde_json::from_slice(&bytes).map_err(|e| JsValue::from_str(&format!("invalid ticket JSON: {e}")))?;

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

    let parsed = ParsedTicket {
        endpoint_id,
        direct_addresses,
        ws_port,
        relay_url,
    };

    serde_wasm_bindgen::to_value(&parsed).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn encode_rpc_request(
    id: Option<String>,
    command_type: &str,
    payload_json: Option<String>,
) -> Result<String, JsValue> {
    let mut map = serde_json::Map::new();
    if let Some(req_id) = id {
        map.insert("id".to_string(), Value::String(req_id));
    }
    map.insert("type".to_string(), Value::String(command_type.to_string()));

    if let Some(payload_str) = payload_json
        && !payload_str.trim().is_empty()
    {
        let parsed_payload: Value =
            serde_json::from_str(&payload_str).map_err(|e| JsValue::from_str(&format!("invalid payload JSON: {e}")))?;
        if let Value::Object(obj) = parsed_payload {
            for (k, v) in obj {
                map.insert(k, v);
            }
        }
    }

    let val = Value::Object(map);
    serde_json::to_string(&val).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_rpc_frame(line: &str) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(line.trim())
}

#[derive(Serialize, Deserialize)]
pub struct SplitStreamContent {
    pub text: String,
    pub thinking: Option<String>,
    pub is_thinking_active: bool,
}

#[wasm_bindgen]
pub fn process_stream_content(full_buffer: &str) -> Result<JsValue, JsValue> {
    let thinking_start = full_buffer.find("<thinking>");
    let thinking_end = full_buffer.find("</thinking>");

    let (text, thinking, is_thinking_active) = match (thinking_start, thinking_end) {
        (Some(start), Some(end)) => {
            let before = &full_buffer[..start];
            let inside = &full_buffer[start + 10..end];
            let after = &full_buffer[end + 11..];
            (format!("{before}{after}"), Some(inside.to_string()), false)
        }
        (Some(start), None) => {
            let before = &full_buffer[..start];
            let inside = &full_buffer[start + 10..];
            (before.to_string(), Some(inside.to_string()), true)
        }
        _ => (full_buffer.to_string(), None, false),
    };

    let result = SplitStreamContent {
        text,
        thinking,
        is_thinking_active,
    };
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ticket() {
        let ticket_json = r#"{"node_id":"test-node","ws_port":50051,"addrs":["127.0.0.1:50051"]}"#;
        let b64 = URL_SAFE_NO_PAD.encode(ticket_json);
        let ticket_str = format!("rho_{b64}");
        let raw = extract_ticket_b64(&ticket_str);
        assert_eq!(raw, b64);
    }

    #[test]
    fn test_extract_ticket_b64() {
        assert_eq!(extract_ticket_b64("rho_abc123"), "abc123");
        assert_eq!(extract_ticket_b64("abc123"), "abc123");
        assert_eq!(
            extract_ticket_b64("https://casonadams.github.io/rho/hub/#ticket=rho_abc123&session=sess-1"),
            "abc123"
        );
        assert_eq!(
            extract_ticket_b64("https://casonadams.github.io/rho/hub/#ticket=rho_abc123"),
            "abc123"
        );
        assert_eq!(
            extract_ticket_b64("https://casonadams.github.io/rho/hub/?ticket=rho_abc123&session=sess-1"),
            "abc123"
        );
        assert_eq!(extract_ticket_b64("rho_abc123&session=sess-1"), "abc123");
        assert_eq!(
            extract_ticket_b64("<https://casonadams.github.io/rho/hub/#ticket=rho_abc123&session=sess-1>"),
            "abc123"
        );
    }

    #[test]
    fn test_encode_and_parse_rpc() {
        let encoded = encode_rpc_request(
            Some("1".to_string()),
            "prompt",
            Some(r#"{"message":"hello world"}"#.to_string()),
        )
        .unwrap();
        assert!(encoded.contains("\"type\":\"prompt\""));
        assert!(encoded.contains("\"message\":\"hello world\""));
        assert!(encoded.contains("\"id\":\"1\""));
    }

    #[test]
    fn test_process_stream_thinking() {
        let full = "Hello <thinking>pondering</thinking> Done!";
        let thinking_start = full.find("<thinking>");
        let thinking_end = full.find("</thinking>");
        assert!(thinking_start.is_some());
        assert!(thinking_end.is_some());
    }
}
