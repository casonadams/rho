use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use iroh::{EndpointId, RelayUrl};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollabTicket {
    pub endpoint_id: EndpointId,
    pub relay_url: Option<RelayUrl>,
    pub is_view_only: bool,
    pub secret: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum TicketError {
    #[error("invalid URI scheme (expected rho://)")]
    InvalidScheme,
    #[error("missing host endpoint id")]
    MissingEndpointId,
    #[error("invalid endpoint id: {0}")]
    InvalidEndpointId(String),
    #[error("invalid relay url: {0}")]
    InvalidRelayUrl(String),
    #[error("missing fragment secret")]
    MissingSecret,
    #[error("invalid base64 secret")]
    InvalidSecretBase64,
    #[error("invalid secret length (expected 32 bytes, got {0})")]
    InvalidSecretLength(usize),
}

impl CollabTicket {
    #[must_use]
    pub fn new(endpoint_id: EndpointId, relay_url: Option<RelayUrl>, is_view_only: bool, secret: [u8; 32]) -> Self {
        Self {
            endpoint_id,
            relay_url,
            is_view_only,
            secret,
        }
    }

    pub fn parse(s: &str) -> std::result::Result<Self, TicketError> {
        s.parse()
    }

    #[must_use]
    pub fn to_endpoint_addr(&self) -> iroh::EndpointAddr {
        let addr = iroh::EndpointAddr::new(self.endpoint_id);
        if let Some(ref relay) = self.relay_url {
            addr.with_relay_url(relay.clone())
        } else {
            addr
        }
    }

    #[must_use]
    pub fn to_uri(&self) -> String {
        let b64_secret = URL_SAFE_NO_PAD.encode(self.secret);
        let mut uri = format!("rho://{}", self.endpoint_id);
        let mut query_parts = Vec::new();

        if let Some(relay) = &self.relay_url {
            query_parts.push(format!("relay={relay}"));
        }
        if self.is_view_only {
            query_parts.push("view=1".to_string());
        }

        if !query_parts.is_empty() {
            uri.push('?');
            uri.push_str(&query_parts.join("&"));
        }

        uri.push('#');
        uri.push_str(&b64_secret);
        uri
    }
}

impl fmt::Display for CollabTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uri())
    }
}

fn parse_secret_fragment(fragment: &str) -> Result<[u8; 32], TicketError> {
    let secret_bytes = URL_SAFE_NO_PAD
        .decode(fragment)
        .map_err(|_| TicketError::InvalidSecretBase64)?;

    if secret_bytes.len() != 32 {
        return Err(TicketError::InvalidSecretLength(secret_bytes.len()));
    }

    let mut secret = [0u8; 32];
    secret.copy_from_slice(&secret_bytes);
    Ok(secret)
}

fn parse_query_params(query: Option<&str>) -> Result<(Option<RelayUrl>, bool), TicketError> {
    let mut relay_url = None;
    let mut is_view_only = false;

    let Some(query_str) = query else {
        return Ok((relay_url, is_view_only));
    };

    for param in query_str.split('&') {
        let Some((k, v)) = param.split_once('=') else {
            continue;
        };
        match k {
            "relay" => {
                let url = RelayUrl::from_str(v).map_err(|e| TicketError::InvalidRelayUrl(e.to_string()))?;
                relay_url = Some(url);
            }
            "view" => {
                is_view_only = v == "1" || v.eq_ignore_ascii_case("true");
            }
            _ => {}
        }
    }

    Ok((relay_url, is_view_only))
}

impl FromStr for CollabTicket {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let without_scheme = s.strip_prefix("rho://").ok_or(TicketError::InvalidScheme)?;
        let (before_fragment, fragment) = without_scheme.split_once('#').ok_or(TicketError::MissingSecret)?;
        let secret = parse_secret_fragment(fragment)?;

        let (host_str, query) = match before_fragment.split_once('?') {
            Some((h, q)) => (h, Some(q)),
            None => (before_fragment, None),
        };

        if host_str.is_empty() {
            return Err(TicketError::MissingEndpointId);
        }

        let endpoint_id = EndpointId::from_str(host_str).map_err(|e| TicketError::InvalidEndpointId(e.to_string()))?;
        let (relay_url, is_view_only) = parse_query_params(query)?;

        Ok(Self {
            endpoint_id,
            relay_url,
            is_view_only,
            secret,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn test_ticket_roundtrip() {
        let secret_key = SecretKey::generate();
        let endpoint_id = secret_key.public();
        let secret = [42u8; 32];
        let relay_url = RelayUrl::from_str("https://relay.iroh.network").ok();

        let ticket = CollabTicket::new(endpoint_id, relay_url.clone(), false, secret);
        let uri = ticket.to_uri();
        let parsed = CollabTicket::from_str(&uri).expect("failed to parse URI");

        assert_eq!(parsed, ticket);
        assert!(!parsed.is_view_only);

        let view_ticket = CollabTicket::new(endpoint_id, relay_url, true, secret);
        let view_uri = view_ticket.to_uri();
        let parsed_view = CollabTicket::from_str(&view_uri).expect("failed to parse view URI");

        assert_eq!(parsed_view, view_ticket);
        assert!(parsed_view.is_view_only);
    }
}
