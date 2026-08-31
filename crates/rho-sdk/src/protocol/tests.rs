use super::*;
use crate::contract::*;

fn request_id(value: &str) -> RequestId {
    RequestId::new(value).unwrap()
}

#[test]
fn handshake_has_stable_golden_ndjson() {
    let envelope = Envelope::new(
        request_id("req-1"),
        ProtocolMessage::HandshakeRequest {
            supported_versions: vec![1],
            plugin_config: None,
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
                    plugin_config: None,
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
