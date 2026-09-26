use crate::collab::crypto::{CapabilityLevel, CollabSecret};
use crate::error::{AppError, Result};
use crate::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncWrite};

pub const COLLAB_ALPN: &[u8] = b"rho/collab/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeChallenge {
    pub nonce: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub role: CapabilityLevel,
    pub auth_mac: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeAck {
    pub accepted: bool,
    pub role: CapabilityLevel,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HandshakeMessage {
    Challenge(HandshakeChallenge),
    Response(HandshakeResponse),
    Ack(HandshakeAck),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollabSnapshot {
    pub turns: Vec<serde_json::Value>,
    pub current_state: String,
}

impl CollabSnapshot {
    #[must_use]
    pub fn new(turns: Vec<serde_json::Value>, current_state: impl Into<String>) -> Self {
        Self {
            turns,
            current_state: current_state.into(),
        }
    }

    #[must_use]
    pub fn to_rpc_event(&self) -> crate::rpc::protocol::RpcEvent {
        crate::rpc::protocol::RpcEvent::CollabSnapshot {
            turns: self.turns.clone(),
            current_state: self.current_state.clone(),
        }
    }
}

impl From<CollabSnapshot> for crate::rpc::protocol::RpcEvent {
    fn from(snapshot: CollabSnapshot) -> Self {
        crate::rpc::protocol::RpcEvent::CollabSnapshot {
            turns: snapshot.turns,
            current_state: snapshot.current_state,
        }
    }
}

impl TryFrom<crate::rpc::protocol::RpcEvent> for CollabSnapshot {
    type Error = AppError;

    fn try_from(event: crate::rpc::protocol::RpcEvent) -> std::result::Result<Self, Self::Error> {
        match event {
            crate::rpc::protocol::RpcEvent::CollabSnapshot { turns, current_state } => {
                Ok(Self { turns, current_state })
            }
            _ => Err(AppError::Session("Expected CollabSnapshot event".into())),
        }
    }
}

pub async fn perform_host_handshake<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut JsonLinesReader<R>,
    writer: &mut JsonLinesWriter<W>,
    secret: &CollabSecret,
) -> Result<CapabilityLevel> {
    let rng = SystemRandom::new();
    let mut nonce = [0u8; 32];
    rng.fill(&mut nonce)
        .map_err(|_| AppError::Provider("RNG failure generating handshake challenge".into()))?;

    let challenge = HandshakeMessage::Challenge(HandshakeChallenge { nonce });
    writer.write_message(&challenge).await?;

    let response: HandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Auth("Peer disconnected before sending handshake response".into()))?;

    let HandshakeMessage::Response(resp) = response else {
        return Err(AppError::Auth(
            "Unexpected message during handshake (expected response)".into(),
        ));
    };

    let read_key = secret
        .derive_read_key()
        .map_err(|_| AppError::Auth("Failed to derive read key".into()))?;
    let write_key = secret
        .derive_write_key()
        .map_err(|_| AppError::Auth("Failed to derive write key".into()))?;

    let valid = match resp.role {
        CapabilityLevel::Full => CollabSecret::verify_challenge(&write_key, &nonce, &resp.auth_mac),
        CapabilityLevel::ViewOnly => CollabSecret::verify_challenge(&read_key, &nonce, &resp.auth_mac),
    };

    if !valid {
        let ack = HandshakeMessage::Ack(HandshakeAck {
            accepted: false,
            role: resp.role,
            message: Some("Invalid HMAC proof of possession".into()),
        });
        let _ = writer.write_message(&ack).await;
        return Err(AppError::Auth(
            "Collab handshake authentication failed: invalid MAC".into(),
        ));
    }

    let ack = HandshakeMessage::Ack(HandshakeAck {
        accepted: true,
        role: resp.role,
        message: None,
    });
    writer.write_message(&ack).await?;

    Ok(resp.role)
}

pub async fn perform_guest_handshake<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut JsonLinesReader<R>,
    writer: &mut JsonLinesWriter<W>,
    key_material: &[u8; 32],
    is_seed: bool,
    desired_role: CapabilityLevel,
) -> Result<CapabilityLevel> {
    let msg: HandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Auth("Host disconnected before sending handshake challenge".into()))?;

    let HandshakeMessage::Challenge(challenge) = msg else {
        return Err(AppError::Auth(
            "Unexpected message during handshake (expected challenge)".into(),
        ));
    };

    let effective_key = match (is_seed, desired_role) {
        (true, CapabilityLevel::Full) => {
            let secret = CollabSecret::from_bytes(*key_material);
            secret
                .derive_write_key()
                .map_err(|_| AppError::Auth("Failed to derive write key from seed".into()))?
        }
        (true, CapabilityLevel::ViewOnly) => {
            let secret = CollabSecret::from_bytes(*key_material);
            secret
                .derive_read_key()
                .map_err(|_| AppError::Auth("Failed to derive read key from seed".into()))?
        }
        (false, CapabilityLevel::ViewOnly) => *key_material,
        (false, CapabilityLevel::Full) => {
            return Err(AppError::Auth(
                "Cannot request Full capability without master seed".into(),
            ));
        }
    };

    let auth_mac = CollabSecret::sign_challenge(&effective_key, &challenge.nonce);
    let response = HandshakeMessage::Response(HandshakeResponse {
        role: desired_role,
        auth_mac,
    });
    writer.write_message(&response).await?;

    let ack_msg: HandshakeMessage = reader
        .read_message()
        .await?
        .ok_or_else(|| AppError::Auth("Host disconnected before sending handshake ack".into()))?;

    let HandshakeMessage::Ack(ack) = ack_msg else {
        return Err(AppError::Auth(
            "Unexpected message during handshake (expected ack)".into(),
        ));
    };

    if !ack.accepted {
        let msg = ack.message.unwrap_or_else(|| "rejected by host".into());
        return Err(AppError::Auth(format!("Handshake rejected: {msg}")));
    }

    Ok(ack.role)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_handshake_full_capability() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let (host_stream, guest_stream) = duplex(1024);

        let (host_read, host_write) = tokio::io::split(host_stream);
        let mut host_reader = JsonLinesReader::new(tokio::io::BufReader::new(host_read));
        let mut host_writer = JsonLinesWriter::new(host_write);

        let (guest_read, guest_write) = tokio::io::split(guest_stream);
        let mut guest_reader = JsonLinesReader::new(tokio::io::BufReader::new(guest_read));
        let mut guest_writer = JsonLinesWriter::new(guest_write);

        let host_fut = perform_host_handshake(&mut host_reader, &mut host_writer, &secret);
        let guest_fut = perform_guest_handshake(
            &mut guest_reader,
            &mut guest_writer,
            secret.seed(),
            true,
            CapabilityLevel::Full,
        );

        let (host_role, guest_role) = tokio::try_join!(host_fut, guest_fut).expect("handshake failed");
        assert_eq!(host_role, CapabilityLevel::Full);
        assert_eq!(guest_role, CapabilityLevel::Full);
    }

    #[tokio::test]
    async fn test_handshake_view_only_with_read_key() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let read_key = secret.derive_read_key().expect("derived read key");
        let (host_stream, guest_stream) = duplex(1024);

        let (host_read, host_write) = tokio::io::split(host_stream);
        let mut host_reader = JsonLinesReader::new(tokio::io::BufReader::new(host_read));
        let mut host_writer = JsonLinesWriter::new(host_write);

        let (guest_read, guest_write) = tokio::io::split(guest_stream);
        let mut guest_reader = JsonLinesReader::new(tokio::io::BufReader::new(guest_read));
        let mut guest_writer = JsonLinesWriter::new(guest_write);

        let host_fut = perform_host_handshake(&mut host_reader, &mut host_writer, &secret);
        let guest_fut = perform_guest_handshake(
            &mut guest_reader,
            &mut guest_writer,
            &read_key,
            false,
            CapabilityLevel::ViewOnly,
        );

        let (host_role, guest_role) = tokio::try_join!(host_fut, guest_fut).expect("handshake failed");
        assert_eq!(host_role, CapabilityLevel::ViewOnly);
        assert_eq!(guest_role, CapabilityLevel::ViewOnly);
    }

    #[tokio::test]
    async fn test_handshake_view_only_cannot_forge_full() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let read_key = secret.derive_read_key().expect("derived read key");
        let (host_stream, guest_stream) = duplex(1024);

        let (host_read, host_write) = tokio::io::split(host_stream);
        let mut host_reader = JsonLinesReader::new(tokio::io::BufReader::new(host_read));
        let mut host_writer = JsonLinesWriter::new(host_write);

        let (guest_read, guest_write) = tokio::io::split(guest_stream);
        let mut guest_reader = JsonLinesReader::new(tokio::io::BufReader::new(guest_read));
        let mut guest_writer = JsonLinesWriter::new(guest_write);

        let host_fut = perform_host_handshake(&mut host_reader, &mut host_writer, &secret);
        let guest_adversary_fut = async {
            let msg: HandshakeMessage = guest_reader
                .read_message()
                .await?
                .ok_or_else(|| AppError::Auth("Host disconnected".into()))?;
            let HandshakeMessage::Challenge(challenge) = msg else {
                return Err(AppError::Auth("Expected challenge".into()));
            };
            let forged_mac = CollabSecret::sign_challenge(&read_key, &challenge.nonce);
            guest_writer
                .write_message(&HandshakeMessage::Response(HandshakeResponse {
                    role: CapabilityLevel::Full,
                    auth_mac: forged_mac,
                }))
                .await?;
            let ack_msg: HandshakeMessage = guest_reader
                .read_message()
                .await?
                .ok_or_else(|| AppError::Auth("Host disconnected".into()))?;
            let HandshakeMessage::Ack(ack) = ack_msg else {
                return Err(AppError::Auth("Expected ack".into()));
            };
            if !ack.accepted {
                return Err(AppError::Auth("Handshake rejected as expected".into()));
            }
            Ok(())
        };

        let (host_res, guest_res) = tokio::join!(host_fut, guest_adversary_fut);
        assert!(host_res.is_err());
        assert!(guest_res.is_err());
    }

    #[tokio::test]
    async fn test_handshake_invalid_signature_rejected() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let bogus_key = [0x99u8; 32];
        let (host_stream, guest_stream) = duplex(1024);

        let (host_read, host_write) = tokio::io::split(host_stream);
        let mut host_reader = JsonLinesReader::new(tokio::io::BufReader::new(host_read));
        let mut host_writer = JsonLinesWriter::new(host_write);

        let (guest_read, guest_write) = tokio::io::split(guest_stream);
        let mut guest_reader = JsonLinesReader::new(tokio::io::BufReader::new(guest_read));
        let mut guest_writer = JsonLinesWriter::new(guest_write);

        let host_fut = perform_host_handshake(&mut host_reader, &mut host_writer, &secret);
        let guest_fut = perform_guest_handshake(
            &mut guest_reader,
            &mut guest_writer,
            &bogus_key,
            false,
            CapabilityLevel::ViewOnly,
        );

        let (host_res, guest_res) = tokio::join!(host_fut, guest_fut);
        assert!(host_res.is_err());
        assert!(guest_res.is_err());
    }

    #[test]
    fn test_collab_snapshot_roundtrip() {
        let snapshot = CollabSnapshot::new(vec![serde_json::json!({"turn": 1, "prompt": "hello world"})], "running");
        let event = snapshot.to_rpc_event();
        let parsed = CollabSnapshot::try_from(event).expect("roundtrip conversion");
        assert_eq!(parsed, snapshot);
    }
}
