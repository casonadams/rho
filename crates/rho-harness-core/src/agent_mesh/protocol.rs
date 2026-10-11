use crate::agent_mesh::crypto::AgentSecret;
use crate::agent_mesh::ticket::AgentTicket;
use crate::agent_mesh::types::AgentCapability;
use crate::error::{AppError, Result};
use crate::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncWrite};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHandshakeChallenge {
    pub nonce: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHandshakeResponse {
    pub capability: AgentCapability,
    pub auth_mac: [u8; 32],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHandshakeAck {
    pub accepted: bool,
    pub granted_capability: AgentCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentHandshakeMessage {
    Challenge(AgentHandshakeChallenge),
    Response(AgentHandshakeResponse),
    Ack(AgentHandshakeAck),
}

pub fn enforce_tool_permission(cap: AgentCapability, tool_name: &str) -> Result<()> {
    if cap.is_tool_allowed(tool_name) {
        Ok(())
    } else {
        Err(AppError::Tool(format!(
            "Tool '{tool_name}' is forbidden under agent capability '{cap}'"
        )))
    }
}

fn create_handshake_nonce() -> Result<[u8; 32]> {
    let mut nonce = [0u8; 32];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| AppError::Session("failed to generate handshake nonce".into()))?;
    Ok(nonce)
}

fn verify_client_response_mac(secret: &AgentSecret, response: &AgentHandshakeResponse, nonce: &[u8; 32]) -> Result<()> {
    let key = secret
        .derive_key(response.capability)
        .map_err(|_| AppError::Auth("key derivation failed".into()))?;
    if AgentSecret::verify_signature(&key, nonce, &response.auth_mac) {
        Ok(())
    } else {
        Err(AppError::Auth("invalid agent handshake authentication".into()))
    }
}

pub async fn perform_worker_handshake<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut JsonLinesReader<R>,
    writer: &mut JsonLinesWriter<W>,
    worker_secret: &AgentSecret,
) -> Result<(AgentCapability, Option<String>)> {
    let nonce = create_handshake_nonce()?;
    writer
        .write_message(&AgentHandshakeMessage::Challenge(AgentHandshakeChallenge { nonce }))
        .await?;

    let incoming: AgentHandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Session("connection closed waiting for client handshake".into()))?;

    let resp = match incoming {
        AgentHandshakeMessage::Response(r) => r,
        _ => return Err(AppError::Session("expected handshake response message".into())),
    };

    if let Err(e) = verify_client_response_mac(worker_secret, &resp, &nonce) {
        let ack = AgentHandshakeMessage::Ack(AgentHandshakeAck {
            accepted: false,
            granted_capability: AgentCapability::ReadOnly,
            message: Some(e.to_string()),
        });
        let _ = writer.write_message(&ack).await;
        return Err(e);
    }

    let ack = AgentHandshakeMessage::Ack(AgentHandshakeAck {
        accepted: true,
        granted_capability: resp.capability,
        message: None,
    });
    writer.write_message(&ack).await?;

    Ok((resp.capability, resp.client_id))
}

pub async fn perform_client_handshake<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut JsonLinesReader<R>,
    writer: &mut JsonLinesWriter<W>,
    ticket: &AgentTicket,
    client_id: Option<String>,
) -> Result<AgentCapability> {
    let challenge_msg: AgentHandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Session("connection closed waiting for challenge".into()))?;

    let challenge = match challenge_msg {
        AgentHandshakeMessage::Challenge(c) => c,
        _ => return Err(AppError::Session("expected handshake challenge message".into())),
    };

    let auth_mac = AgentSecret::sign_challenge(&ticket.secret, &challenge.nonce);
    let resp = AgentHandshakeMessage::Response(AgentHandshakeResponse {
        capability: ticket.capability,
        auth_mac,
        client_id,
    });
    writer.write_message(&resp).await?;

    let ack_msg: AgentHandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Session("connection closed waiting for handshake ack".into()))?;

    let ack = match ack_msg {
        AgentHandshakeMessage::Ack(a) => a,
        _ => return Err(AppError::Session("expected handshake ack message".into())),
    };

    if !ack.accepted {
        let msg = ack.message.unwrap_or_else(|| "handshake rejected by worker".into());
        return Err(AppError::Auth(msg));
    }

    Ok(ack.granted_capability)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn execute_handshake_pair(
        worker_secret: &AgentSecret,
        ticket: &AgentTicket,
        client_tag: Option<&str>,
    ) -> (Result<(AgentCapability, Option<String>)>, Result<AgentCapability>) {
        let (worker_read, client_write) = tokio::io::duplex(4096);
        let (client_read, worker_write) = tokio::io::duplex(4096);

        let mut w_reader = JsonLinesReader::new(tokio::io::BufReader::new(worker_read));
        let mut w_writer = JsonLinesWriter::new(worker_write);
        let mut c_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_read));
        let mut c_writer = JsonLinesWriter::new(client_write);

        let w_sec = worker_secret.clone();
        let t_clone = ticket.clone();
        let tag = client_tag.map(ToString::to_string);

        let w_handle =
            tokio::spawn(async move { perform_worker_handshake(&mut w_reader, &mut w_writer, &w_sec).await });
        let c_handle =
            tokio::spawn(async move { perform_client_handshake(&mut c_reader, &mut c_writer, &t_clone, tag).await });

        let (w_res, c_res) = tokio::join!(w_handle, c_handle);
        (w_res.unwrap(), c_res.unwrap())
    }

    #[tokio::test]
    async fn test_mutual_handshake_flow() {
        let worker_secret = AgentSecret::generate().unwrap();
        let key = worker_secret.derive_key(AgentCapability::Propose).unwrap();
        let ep = iroh::SecretKey::generate().public();
        let good_ticket = AgentTicket::new(ep, None, AgentCapability::Propose, key);

        let (w_ok, c_ok) = execute_handshake_pair(&worker_secret, &good_ticket, Some("client-node")).await;
        let (cap, id) = w_ok.unwrap();
        assert_eq!(cap, AgentCapability::Propose);
        assert_eq!(id.as_deref(), Some("client-node"));
        assert_eq!(c_ok.unwrap(), AgentCapability::Propose);

        let bad_ticket = AgentTicket::new(ep, None, AgentCapability::Full, [1; 32]);
        let (w_bad, c_bad) = execute_handshake_pair(&worker_secret, &bad_ticket, None).await;
        assert!(w_bad.is_err());
        assert!(c_bad.is_err());
    }

    #[test]
    fn test_enforce_tool_permission() {
        assert!(enforce_tool_permission(AgentCapability::ReadOnly, "read").is_ok());
        assert!(enforce_tool_permission(AgentCapability::ReadOnly, "write").is_err());
        assert!(enforce_tool_permission(AgentCapability::Propose, "write").is_ok());
        assert!(enforce_tool_permission(AgentCapability::Propose, "bash").is_err());
        assert!(enforce_tool_permission(AgentCapability::Full, "bash").is_ok());
    }
}
