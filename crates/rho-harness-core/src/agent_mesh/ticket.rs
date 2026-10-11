use crate::agent_mesh::types::AgentCapability;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use iroh::{EndpointId, RelayUrl};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTicket {
    pub endpoint_id: EndpointId,
    pub relay_url: Option<RelayUrl>,
    pub capability: AgentCapability,
    pub secret: [u8; 32],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentTicketError {
    #[error("invalid URI scheme (expected rho-agent://)")]
    InvalidScheme,
    #[error("missing endpoint id in ticket")]
    MissingEndpointId,
    #[error("invalid endpoint id: {0}")]
    InvalidEndpointId(String),
    #[error("invalid relay url: {0}")]
    InvalidRelayUrl(String),
    #[error("missing or invalid capability in query")]
    InvalidCapability(String),
    #[error("missing fragment secret")]
    MissingSecret,
    #[error("invalid base64 secret")]
    InvalidSecretBase64,
    #[error("invalid secret length (expected 32 bytes, got {0})")]
    InvalidSecretLength(usize),
}

impl AgentTicket {
    #[must_use]
    pub const fn new(
        endpoint_id: EndpointId,
        relay_url: Option<RelayUrl>,
        capability: AgentCapability,
        secret: [u8; 32],
    ) -> Self {
        Self {
            endpoint_id,
            relay_url,
            capability,
            secret,
        }
    }

    pub fn parse(s: &str) -> Result<Self, AgentTicketError> {
        s.parse()
    }

    #[must_use]
    pub fn to_endpoint_addr(&self) -> iroh::EndpointAddr {
        match &self.relay_url {
            Some(relay) => iroh::EndpointAddr::new(self.endpoint_id).with_relay_url(relay.clone()),
            None => iroh::EndpointAddr::new(self.endpoint_id),
        }
    }

    #[must_use]
    pub fn to_uri(&self) -> String {
        let b64 = URL_SAFE_NO_PAD.encode(self.secret);
        let relay_part = self
            .relay_url
            .as_ref()
            .map(|r| format!("&relay={r}"))
            .unwrap_or_default();
        format!(
            "rho-agent://{}?cap={}{relay_part}#{b64}",
            self.endpoint_id, self.capability
        )
    }
}

impl fmt::Display for AgentTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_uri())
    }
}

fn parse_ticket_query(query_str: &str) -> Result<(AgentCapability, Option<RelayUrl>), AgentTicketError> {
    let mut capability = None;
    let mut relay_url = None;

    for part in query_str.split('&') {
        if let Some(val) = part.strip_prefix("cap=") {
            let cap =
                AgentCapability::from_str(val).map_err(|_| AgentTicketError::InvalidCapability(val.to_string()))?;
            capability = Some(cap);
        } else if let Some(val) = part.strip_prefix("relay=") {
            let url = RelayUrl::from_str(val).map_err(|e| AgentTicketError::InvalidRelayUrl(e.to_string()))?;
            relay_url = Some(url);
        }
    }

    let cap = capability.ok_or_else(|| AgentTicketError::InvalidCapability("missing cap parameter".into()))?;
    Ok((cap, relay_url))
}

impl From<crate::collab::ticket::TicketError> for AgentTicketError {
    fn from(err: crate::collab::ticket::TicketError) -> Self {
        match err {
            crate::collab::ticket::TicketError::InvalidScheme => Self::InvalidScheme,
            crate::collab::ticket::TicketError::MissingEndpointId => Self::MissingEndpointId,
            crate::collab::ticket::TicketError::InvalidEndpointId(e) => Self::InvalidEndpointId(e),
            crate::collab::ticket::TicketError::InvalidRelayUrl(r) => Self::InvalidRelayUrl(r),
            crate::collab::ticket::TicketError::MissingSecret => Self::MissingSecret,
            crate::collab::ticket::TicketError::InvalidSecretBase64 => Self::InvalidSecretBase64,
            crate::collab::ticket::TicketError::InvalidSecretLength(n) => Self::InvalidSecretLength(n),
        }
    }
}

impl FromStr for AgentTicket {
    type Err = AgentTicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = crate::collab::ticket::parse_ticket_uri(s, "rho-agent://")?;
        let query_str = parts
            .query
            .ok_or_else(|| AgentTicketError::InvalidCapability("missing query parameters".into()))?;
        let (capability, relay_url) = parse_ticket_query(query_str)?;
        Ok(Self::new(parts.endpoint_id, relay_url, capability, parts.secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_ticket_roundtrip() {
        let endpoint_id = iroh::SecretKey::generate().public();
        let relay_url = Some(RelayUrl::from_str("https://relay.example.com").unwrap());
        let secret = [7u8; 32];
        let ticket = AgentTicket::new(endpoint_id, relay_url.clone(), AgentCapability::Propose, secret);

        let uri = ticket.to_uri();
        assert!(uri.starts_with("rho-agent://"));
        assert!(uri.contains("cap=propose"));
        assert!(uri.contains("relay="));

        let parsed: AgentTicket = uri.parse().expect("valid ticket URI");
        assert_eq!(parsed.endpoint_id, endpoint_id);
        assert_eq!(parsed.relay_url, relay_url);
        assert_eq!(parsed.capability, AgentCapability::Propose);
        assert_eq!(parsed.secret, secret);
    }

    #[test]
    fn test_agent_ticket_without_relay() {
        let endpoint_id = iroh::SecretKey::generate().public();
        let secret = [99u8; 32];
        let ticket = AgentTicket::new(endpoint_id, None, AgentCapability::ReadOnly, secret);

        let uri = ticket.to_uri();
        assert!(!uri.contains("relay="));

        let parsed = AgentTicket::parse(&uri).expect("parses ticket without relay");
        assert_eq!(parsed.capability, AgentCapability::ReadOnly);
        assert_eq!(parsed.relay_url, None);
        assert_eq!(parsed.secret, secret);
    }

    #[test]
    fn test_agent_ticket_invalid_scheme() {
        let res = AgentTicket::parse("http://example.com");
        assert_eq!(res.unwrap_err(), AgentTicketError::InvalidScheme);
    }
}
