use crate::capability::{CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION};
use crate::contract::{
    AuthenticationRequest, AuthenticationResponse, CapabilityDescriptor, CommandInvocationRequest,
    CommandInvocationResponse, ContextRequest, ContextResponse, LifecycleEvent, PermissionDecision, ProviderRequest,
    ProviderStreamEvent, RequestedOperation, SkillAsset, ToolInvocationRequest, ToolInvocationResponse,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::BTreeSet;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    HandshakeRequest {
        supported_versions: Vec<u32>,
    },
    DiscoveryRequest,
    InvocationRequest {
        capability_id: CapabilityId,
        invocation: InvocationRequest,
    },
    StreamEvent {
        event: StreamEvent,
    },
    CancelRequest {
        target_request_id: RequestId,
    },
    TerminalResponse {
        result: TerminalResult,
    },
    ErrorResponse {
        error: StructuredError,
    },
}

impl ProtocolMessage {
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::TerminalResponse { .. } | Self::ErrorResponse { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum InvocationRequest {
    ProviderStream(ProviderRequest),
    ProviderAuthenticate(AuthenticationRequest),
    Tool(ToolInvocationRequest),
    Permission(RequestedOperation),
    Command(CommandInvocationRequest),
    Lifecycle(LifecycleEvent),
    Skills,
    Context(ContextRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    Provider(ProviderStreamEvent),
    Progress { message: String },
    CommandOutput { content: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "response", rename_all = "snake_case")]
pub enum TerminalResult {
    Handshake {
        selected_version: u32,
    },
    Discovery {
        manifest: CapabilityManifest,
        capabilities: Vec<CapabilityDescriptor>,
    },
    ProviderAuthenticated(AuthenticationResponse),
    Tool(ToolInvocationResponse),
    Permission(PermissionDecision),
    Command(CommandInvocationResponse),
    Lifecycle,
    Skills(Vec<SkillAsset>),
    Context(ContextResponse),
    StreamCompleted,
    Cancelled,
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolViolation {
    #[error("protocol line exceeds the configured size limit")]
    LineTooLarge,
    #[error("protocol message exceeds the configured size limit")]
    MessageTooLarge,
    #[error("protocol result exceeds the configured size limit")]
    ResultTooLarge,
    #[error("structured protocol error exceeds the configured size limit")]
    ErrorTooLarge,
    #[error("protocol input is not valid UTF-8")]
    InvalidUtf8,
    #[error("protocol input must contain exactly one NDJSON object")]
    InvalidFraming,
    #[error("protocol message is malformed")]
    Malformed,
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u32),
    #[error("request identifier is invalid")]
    InvalidRequestId,
    #[error("response request identifier does not match the active request")]
    CorrelationMismatch,
    #[error("stream event arrived after a terminal response")]
    EventAfterTerminal,
    #[error("response sequence contains more than one terminal response")]
    DuplicateTerminal,
    #[error("response sequence did not contain a terminal response")]
    MissingTerminal,
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

fn validate_encoded_size(envelope: &Envelope, encoded_len: usize) -> Result<(), ProtocolViolation> {
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

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

#[derive(Debug)]
pub struct ResponseSequenceValidator {
    request_id: RequestId,
    terminal: bool,
    event_kinds: BTreeSet<&'static str>,
}

impl ResponseSequenceValidator {
    pub fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            terminal: false,
            event_kinds: BTreeSet::new(),
        }
    }

    pub fn accept(&mut self, envelope: &Envelope) -> Result<(), ProtocolViolation> {
        if envelope.request_id != self.request_id {
            return Err(ProtocolViolation::CorrelationMismatch);
        }
        if self.terminal {
            return if envelope.message.is_terminal() {
                Err(ProtocolViolation::DuplicateTerminal)
            } else {
                Err(ProtocolViolation::EventAfterTerminal)
            };
        }
        match &envelope.message {
            ProtocolMessage::StreamEvent { event } => {
                self.event_kinds.insert(match event {
                    StreamEvent::Provider(_) => "provider",
                    StreamEvent::Progress { .. } => "progress",
                    StreamEvent::CommandOutput { .. } => "command_output",
                });
            }
            message if message.is_terminal() => self.terminal = true,
            _ => return Err(ProtocolViolation::Malformed),
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), ProtocolViolation> {
        if self.terminal {
            Ok(())
        } else {
            Err(ProtocolViolation::MissingTerminal)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id(value: &str) -> RequestId {
        RequestId::new(value).unwrap()
    }

    #[test]
    fn handshake_has_stable_golden_ndjson() {
        let envelope = Envelope::new(
            request_id("req-1"),
            ProtocolMessage::HandshakeRequest {
                supported_versions: vec![1],
            },
        );
        let encoded = String::from_utf8(encode_line(&envelope).unwrap()).unwrap();
        assert_eq!(
            encoded,
            "{\"protocol_version\":1,\"request_id\":\"req-1\",\"type\":\"handshake_request\",\"supported_versions\":[1]}\n"
        );
        assert_eq!(decode_line(encoded.as_bytes()).unwrap(), envelope);
    }

    #[test]
    fn correlates_streams_and_requires_one_terminal_response() {
        let id = request_id("stream-1");
        let mut validator = ResponseSequenceValidator::new(id.clone());
        validator
            .accept(&Envelope::new(
                id.clone(),
                ProtocolMessage::StreamEvent {
                    event: StreamEvent::Provider(ProviderStreamEvent::TextDelta {
                        text: "hello".to_string(),
                    }),
                },
            ))
            .unwrap();
        validator
            .accept(&Envelope::new(
                id,
                ProtocolMessage::TerminalResponse {
                    result: TerminalResult::StreamCompleted,
                },
            ))
            .unwrap();
        validator.finish().unwrap();

        let mut missing = ResponseSequenceValidator::new(request_id("missing"));
        missing
            .accept(&Envelope::new(
                request_id("missing"),
                ProtocolMessage::StreamEvent {
                    event: StreamEvent::Progress {
                        message: "working".to_string(),
                    },
                },
            ))
            .unwrap();
        assert_eq!(missing.finish(), Err(ProtocolViolation::MissingTerminal));
    }

    #[test]
    fn rejects_wrong_correlation_and_events_after_terminal() {
        let mut validator = ResponseSequenceValidator::new(request_id("expected"));
        assert_eq!(
            validator.accept(&Envelope::new(
                request_id("other"),
                ProtocolMessage::TerminalResponse {
                    result: TerminalResult::StreamCompleted,
                },
            )),
            Err(ProtocolViolation::CorrelationMismatch)
        );
        validator
            .accept(&Envelope::new(
                request_id("expected"),
                ProtocolMessage::TerminalResponse {
                    result: TerminalResult::StreamCompleted,
                },
            ))
            .unwrap();
        assert_eq!(
            validator.accept(&Envelope::new(
                request_id("expected"),
                ProtocolMessage::StreamEvent {
                    event: StreamEvent::Progress {
                        message: "late".to_string(),
                    },
                },
            )),
            Err(ProtocolViolation::EventAfterTerminal)
        );
    }

    #[test]
    fn cancellation_is_correlated_to_a_target_request() {
        let envelope = Envelope::new(
            request_id("cancel-1"),
            ProtocolMessage::CancelRequest {
                target_request_id: request_id("invoke-1"),
            },
        );
        assert_eq!(decode_line(&encode_line(&envelope).unwrap()).unwrap(), envelope);
    }

    #[test]
    fn rejects_malformed_incompatible_and_oversized_messages() {
        assert_eq!(decode_line(b"not json\n"), Err(ProtocolViolation::Malformed));
        assert_eq!(decode_line(&[0xff, b'\n']), Err(ProtocolViolation::InvalidUtf8));
        assert_eq!(decode_line(b"{}\n{}\n"), Err(ProtocolViolation::InvalidFraming));
        let incompatible = b"{\"protocol_version\":2,\"request_id\":\"r\",\"type\":\"discovery_request\"}\n";
        assert_eq!(decode_line(incompatible), Err(ProtocolViolation::UnsupportedVersion(2)));
        assert_eq!(
            decode_line(&vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 2]),
            Err(ProtocolViolation::LineTooLarge)
        );
    }

    #[test]
    fn enforces_terminal_result_and_structured_error_bounds() {
        let oversized_result = Envelope::new(
            request_id("result"),
            ProtocolMessage::TerminalResponse {
                result: TerminalResult::Tool(ToolInvocationResponse {
                    content: "x".repeat(MAX_PROTOCOL_RESULT_BYTES),
                    is_error: false,
                    structured_content: None,
                }),
            },
        );
        assert_eq!(encode_line(&oversized_result), Err(ProtocolViolation::ResultTooLarge));

        let oversized_error = Envelope::new(
            request_id("error"),
            ProtocolMessage::ErrorResponse {
                error: StructuredError {
                    code: ErrorCode::Internal,
                    message: "x".repeat(MAX_PROTOCOL_ERROR_MESSAGE_BYTES + 1),
                    retryable: false,
                    details: None,
                },
            },
        );
        assert_eq!(encode_line(&oversized_error), Err(ProtocolViolation::ErrorTooLarge));
    }

    #[test]
    fn redacted_errors_never_serialize_sensitive_detail() {
        let secret = "credential-value";
        let error = StructuredError::redacted(ErrorCode::Internal, false);
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(!encoded.contains(secret));
        assert!(encoded.contains("redacted"));

        let public = StructuredError::public(ErrorCode::InvalidRequest, "é".repeat(4096), false);
        assert!(public.message.len() <= MAX_PROTOCOL_ERROR_MESSAGE_BYTES);
    }

    #[test]
    fn context_invocation_and_terminal_response_roundtrip() {
        let envelope = Envelope::new(
            request_id("ctx-1"),
            ProtocolMessage::InvocationRequest {
                capability_id: "context:kiln".parse().unwrap(),
                invocation: InvocationRequest::Context(ContextRequest {
                    prompt: "how to auth".to_string(),
                    context: crate::contract::InvocationContext {
                        session_id: "sess-123".to_string(),
                        working_directory: "/workspace".to_string(),
                        has_interactive_ui: true,
                    },
                    token_budget: Some(2048),
                }),
            },
        );
        let encoded = encode_line(&envelope).unwrap();
        let decoded = decode_line(&encoded).unwrap();
        assert_eq!(decoded, envelope);

        let response_env = Envelope::new(
            request_id("ctx-1"),
            ProtocolMessage::TerminalResponse {
                result: TerminalResult::Context(ContextResponse {
                    snippets: vec![crate::contract::ContextSnippet {
                        source: "auth.md".to_string(),
                        title: Some("Auth Flow".to_string()),
                        content: "Use OAuth".to_string(),
                        score: Some(0.92),
                    }],
                }),
            },
        );
        let resp_encoded = encode_line(&response_env).unwrap();
        let resp_decoded = decode_line(&resp_encoded).unwrap();
        assert_eq!(resp_decoded, response_env);
    }
}
