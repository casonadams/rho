use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use iroh::{Endpoint, SecretKey, endpoint::presets};

pub const RHO_ALPN: &[u8] = b"/rho/rpc/v1";

pub struct RhoEndpoint {
    endpoint: Endpoint,
}

impl RhoEndpoint {
    pub async fn bind(secret: SecretKey, port: Option<u16>) -> Result<Self> {
        let mut builder = Endpoint::builder(presets::N0);
        builder = builder.secret_key(secret);
        builder = builder.alpns(vec![RHO_ALPN.to_vec()]);
        if let Some(p) = port {
            builder = builder.bind_addr(std::net::SocketAddr::from(([0, 0, 0, 0], p)))?;
        }
        let endpoint = builder.bind().await?;
        Ok(Self { endpoint })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn id(&self) -> String {
        self.endpoint.id().to_string()
    }

    pub fn ticket(&self) -> Result<String> {
        self.ticket_with_ws(None)
    }

    pub fn ticket_with_ws(&self, ws_port: Option<u16>) -> Result<String> {
        let mut addr = self.endpoint.addr();
        let port = addr.ip_addrs().next().map(|s| s.port()).unwrap_or(0);
        if port > 0 {
            addr = addr.with_ip_addr(std::net::SocketAddr::from(([127, 0, 0, 1], port)));
        }
        let mut val = serde_json::to_value(&addr).context("failed to serialize endpoint addr")?;
        if let Some(wp) = ws_port
            && let serde_json::Value::Object(ref mut map) = val
        {
            map.insert("ws_port".to_string(), serde_json::json!(wp));
        }
        let json = serde_json::to_vec(&val)?;
        let b64 = URL_SAFE_NO_PAD.encode(json);
        Ok(format!("rho_{b64}"))
    }

    pub fn parse_ticket(ticket_str: &str) -> Result<iroh::EndpointAddr> {
        let raw = ticket_str.strip_prefix("rho_").unwrap_or(ticket_str);
        let bytes = URL_SAFE_NO_PAD.decode(raw).context("invalid base64 ticket")?;
        let addr = serde_json::from_slice(&bytes).context("invalid endpoint addr json")?;
        Ok(addr)
    }

    pub fn pairing_url(ticket: &str) -> String {
        format!("https://casonadams.github.io/rho/hub/#ticket={ticket}")
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

    #[tokio::test]
    async fn test_endpoint_ticket_roundtrip() {
        let secret = SecretKey::generate();
        let endpoint = RhoEndpoint::bind(secret, None).await.unwrap();
        let ticket = endpoint.ticket().unwrap();
        assert!(ticket.starts_with("rho_"));

        let parsed = RhoEndpoint::parse_ticket(&ticket).unwrap();
        assert_eq!(parsed.id, endpoint.endpoint().id());
    }
}
