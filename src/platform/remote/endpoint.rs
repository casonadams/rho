use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointTicket {
    #[serde(alias = "id")]
    pub node_id: String,
    #[serde(default)]
    pub ws_port: Option<u16>,
    #[serde(default)]
    pub addrs: Vec<String>,
}

pub struct RhoEndpoint {
    node_id: String,
    ws_port: Option<u16>,
    addrs: Vec<String>,
}

impl RhoEndpoint {
    pub fn new(node_id: String, ws_port: Option<u16>) -> Self {
        let mut addrs = vec!["127.0.0.1".to_string()];
        if let Some(port) = ws_port {
            addrs = vec![format!("127.0.0.1:{port}")];
        }
        Self {
            node_id,
            ws_port,
            addrs,
        }
    }

    pub fn id(&self) -> &str {
        &self.node_id
    }

    pub fn ws_port(&self) -> Option<u16> {
        self.ws_port
    }

    pub fn ticket(&self) -> Result<String> {
        self.ticket_with_ws(self.ws_port)
    }

    pub fn ticket_with_ws(&self, ws_port: Option<u16>) -> Result<String> {
        let ticket = EndpointTicket {
            node_id: self.node_id.clone(),
            ws_port: ws_port.or(self.ws_port),
            addrs: self.addrs.clone(),
        };
        let json = serde_json::to_vec(&ticket)?;
        let b64 = URL_SAFE_NO_PAD.encode(json);
        Ok(format!("rho_{b64}"))
    }

    pub fn extract_ticket_b64(ticket_str: &str) -> &str {
        let mut raw = ticket_str.trim();
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

    pub fn parse_ticket(ticket_str: &str) -> Result<EndpointTicket> {
        let raw = Self::extract_ticket_b64(ticket_str);
        let bytes = URL_SAFE_NO_PAD.decode(raw).context("invalid base64 ticket")?;
        let ticket = serde_json::from_slice(&bytes).context("invalid endpoint ticket json")?;
        Ok(ticket)
    }

    pub fn pairing_url(ticket: &str) -> String {
        Self::pairing_url_with_session(ticket, None)
    }

    pub fn pairing_url_with_session(ticket: &str, session_id: Option<&str>) -> String {
        if let Some(sid) = session_id {
            format!("https://casonadams.github.io/rho/hub/#ticket={ticket}&session={sid}")
        } else {
            format!("https://casonadams.github.io/rho/hub/#ticket={ticket}")
        }
    }

    pub fn render_qr(text: &str) -> Result<String> {
        use qrcode::QrCode;
        use qrcode::render::unicode::Dense1x2;

        let code = QrCode::new(text.as_bytes()).context("failed to generate QR code")?;
        let image = code.render::<Dense1x2>().build();
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_generation() {
        let qr = RhoEndpoint::render_qr("https://example.com").unwrap();
        assert!(!qr.is_empty());
        assert!(qr.contains('█') || qr.contains('▀') || qr.contains('▄'));
    }

    #[test]
    fn test_endpoint_ticket_roundtrip() {
        let endpoint = RhoEndpoint::new("node-test-123".to_string(), Some(50051));
        let ticket = endpoint.ticket().unwrap();
        assert!(ticket.starts_with("rho_"));

        let parsed = RhoEndpoint::parse_ticket(&ticket).unwrap();
        assert_eq!(parsed.node_id, endpoint.id());
        assert_eq!(parsed.ws_port, Some(50051));

        let url_no_sess = RhoEndpoint::pairing_url(&ticket);
        assert!(!url_no_sess.contains("&session="));

        let url_sess = RhoEndpoint::pairing_url_with_session(&ticket, Some("sess-123"));
        assert!(url_sess.contains("&session=sess-123"));

        let parsed_from_url = RhoEndpoint::parse_ticket(&url_sess).unwrap();
        assert_eq!(parsed_from_url.node_id, endpoint.id());

        let parsed_from_url_no_sess = RhoEndpoint::parse_ticket(&url_no_sess).unwrap();
        assert_eq!(parsed_from_url_no_sess.node_id, endpoint.id());
    }

    #[test]
    fn test_endpoint_ticket_legacy_id_alias() {
        let legacy_json = r#"{"id":"legacy-node-id","ws_port":50052,"addrs":["127.0.0.1:50052"]}"#;
        let b64 = URL_SAFE_NO_PAD.encode(legacy_json.as_bytes());
        let ticket = format!("rho_{b64}");
        let parsed = RhoEndpoint::parse_ticket(&ticket).unwrap();
        assert_eq!(parsed.node_id, "legacy-node-id");
        assert_eq!(parsed.ws_port, Some(50052));
    }
}
