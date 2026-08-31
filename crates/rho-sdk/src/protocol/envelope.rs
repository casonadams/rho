use super::message::ProtocolMessage;
use super::validator::ProtocolViolation;
use crate::capability::PLUGIN_PROTOCOL_VERSION;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::fmt::{Display, Formatter};

pub const MAX_PROTOCOL_LINE_BYTES: usize = 1_048_576;
pub const MAX_PROTOCOL_MESSAGE_BYTES: usize = MAX_PROTOCOL_LINE_BYTES;
pub const MAX_PROTOCOL_RESULT_BYTES: usize = 786_432;
pub const MAX_PROTOCOL_ERROR_MESSAGE_BYTES: usize = 4096;
pub const MAX_PROTOCOL_ERROR_BYTES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(String);

impl RequestId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolViolation> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ProtocolViolation::InvalidRequestId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for RequestId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: u32,
    pub request_id: RequestId,
    #[serde(flatten)]
    pub message: ProtocolMessage,
}

impl Envelope {
    pub fn new(request_id: RequestId, message: ProtocolMessage) -> Self {
        Self {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            request_id,
            message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    CapabilityNotFound,
    ValidationFailed,
    PermissionDenied,
    Cancelled,
    Timeout,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl StructuredError {
    pub fn redacted(code: ErrorCode, retryable: bool) -> Self {
        Self {
            code,
            message: "Plugin operation failed; sensitive details were redacted".to_string(),
            retryable,
            details: None,
        }
    }

    pub fn public(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        let mut message = message.into().replace(['\r', '\n'], " ");
        truncate_utf8(&mut message, MAX_PROTOCOL_ERROR_MESSAGE_BYTES);
        Self {
            code,
            message,
            retryable,
            details: None,
        }
    }
}

pub fn encode_line(envelope: &Envelope) -> Result<Vec<u8>, ProtocolViolation> {
    if envelope.protocol_version != PLUGIN_PROTOCOL_VERSION {
        return Err(ProtocolViolation::UnsupportedVersion(envelope.protocol_version));
    }
    let mut encoded = serde_json::to_vec(envelope).map_err(|_| ProtocolViolation::Malformed)?;
    validate_encoded_size(envelope, encoded.len())?;
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn decode_line(bytes: &[u8]) -> Result<Envelope, ProtocolViolation> {
    if bytes.len() > MAX_PROTOCOL_LINE_BYTES + 1 {
        return Err(ProtocolViolation::LineTooLarge);
    }
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(ProtocolViolation::InvalidFraming);
    }
    let text = std::str::from_utf8(line).map_err(|_| ProtocolViolation::InvalidUtf8)?;
    let envelope: Envelope = serde_json::from_str(text).map_err(|_| ProtocolViolation::Malformed)?;
    if envelope.protocol_version != PLUGIN_PROTOCOL_VERSION {
        return Err(ProtocolViolation::UnsupportedVersion(envelope.protocol_version));
    }
    validate_encoded_size(&envelope, line.len())?;
    Ok(envelope)
}

pub(crate) fn validate_encoded_size(envelope: &Envelope, encoded_len: usize) -> Result<(), ProtocolViolation> {
    if encoded_len > MAX_PROTOCOL_MESSAGE_BYTES {
        return Err(ProtocolViolation::MessageTooLarge);
    }
    if matches!(&envelope.message, ProtocolMessage::TerminalResponse { .. }) {
        let result_len = serde_json::to_vec(&envelope.message)
            .map_err(|_| ProtocolViolation::Malformed)?
            .len();
        if result_len > MAX_PROTOCOL_RESULT_BYTES {
            return Err(ProtocolViolation::ResultTooLarge);
        }
    }
    if let ProtocolMessage::ErrorResponse { error } = &envelope.message {
        let error_len = serde_json::to_vec(error)
            .map_err(|_| ProtocolViolation::Malformed)?
            .len();
        if error.message.len() > MAX_PROTOCOL_ERROR_MESSAGE_BYTES || error_len > MAX_PROTOCOL_ERROR_BYTES {
            return Err(ProtocolViolation::ErrorTooLarge);
        }
    }
    Ok(())
}

pub(crate) fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}
