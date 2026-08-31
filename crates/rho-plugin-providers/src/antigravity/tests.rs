use super::*;
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage, ProviderRequest, ProviderToolDefinition};

#[test]
fn test_pkce_generation_and_challenge() {
    let (verifier, challenge) = generate_pkce();
    assert!(!verifier.is_empty());
    assert!(!challenge.is_empty());
    assert_ne!(verifier, challenge);
}

#[test]
fn test_build_auth_url_contains_required_params() {
    let (verifier, challenge) = generate_pkce();
    let state = generate_state();
    let url = build_auth_url(&challenge, &state);

    assert!(url.starts_with(AUTH_URL));
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("redirect_uri="));
    assert!(url.contains("scope="));
    assert!(url.contains(&state));
    let _ = verifier;
}

#[test]
fn test_stable_project_id_deterministic() {
    let id1 = stable_project_id("test@example.com");
    let id2 = stable_project_id("test@example.com");
    let id3 = stable_project_id("other@example.com");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
    assert_eq!(id1.len(), 36);
}

#[test]
fn test_tokens_serialization_roundtrip() {
    let tokens = AntigravityTokens {
        access_token: "test-access".to_string(),
        refresh_token: "test-refresh".to_string(),
        expires_at: 1234567890,
        project_id: Some("proj-123".to_string()),
        email: Some("test@example.com".to_string()),
    };

    let json = serde_json::to_string(&tokens).unwrap();
    let parsed: AntigravityTokens = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.access_token, "test-access");
    assert_eq!(parsed.refresh_token, "test-refresh");
    assert_eq!(parsed.expires_at, 1234567890);
    assert_eq!(parsed.project_id.as_deref(), Some("proj-123"));
    assert_eq!(parsed.email.as_deref(), Some("test@example.com"));
}

#[test]
fn test_build_antigravity_request_formats_messages_and_tools() {
    let request = ProviderRequest {
        model: "gemini-3.7-flash".to_string(),
        messages: vec![
            ModelMessage {
                role: MessageRole::System,
                content: vec![MessageContent::Text {
                    text: "You are a helper".to_string(),
                }],
            },
            ModelMessage {
                role: MessageRole::User,
                content: vec![MessageContent::Text {
                    text: "Hello world".to_string(),
                }],
            },
        ],
        credential: None,
        max_output_tokens: Some(4096),
        tools: vec![ProviderToolDefinition {
            id: CapabilityId::new(rho_sdk::capability::CapabilityKind::Tool, "read").unwrap(),
            description: "Read file".to_string(),
            argument_schema: serde_json::json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
        }],
    };

    let req = build_antigravity_request(&request, "my-project", "gemini-3.7-flash-high");
    assert_eq!(req.project, "my-project");
    assert_eq!(req.model, "gemini-3.7-flash-high");
    assert_eq!(req.request_type, "agent");
    assert_eq!(req.user_agent, "antigravity");
    assert!(req.request.system_instruction.is_some());
    assert_eq!(req.request.contents.len(), 1);
    assert_eq!(req.request.contents[0].role, "user");
    assert_eq!(req.request.tools.as_ref().unwrap().len(), 1);
    assert_eq!(
        req.request.tools.as_ref().unwrap()[0].function_declarations[0].name,
        "read"
    );
    let schema = req.request.tools.as_ref().unwrap()[0].function_declarations[0]
        .parameters_json_schema
        .as_ref()
        .unwrap();
    assert_eq!(schema["type"], "object");
    // Ensure properties is clean and does not have type="object" injected
    assert!(schema["properties"]["path"].is_object());
    assert_eq!(schema["properties"]["path"]["type"], "string");
    assert!(schema["properties"].get("type").is_none());
}

#[test]
fn test_stream_chunk_parses_wrapped_and_unwrapped_response() {
    use super::types::StreamChunkResponse;

    let wrapped_json = r#"{
        "response": {
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "Hello from Google"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "candidatesTokenCount": 4,
                "totalTokenCount": 16
            }
        }
    }"#;

    let chunk: StreamChunkResponse = serde_json::from_str(wrapped_json).unwrap();
    let candidates = chunk
        .candidates
        .or_else(|| chunk.response.as_ref().and_then(|r| r.candidates.clone()))
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].content.as_ref().unwrap().parts[0].text.as_deref(),
        Some("Hello from Google")
    );

    let unwrapped_json = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Direct candidate"}]
            }
        }]
    }"#;
    let chunk2: StreamChunkResponse = serde_json::from_str(unwrapped_json).unwrap();
    let candidates2 = chunk2
        .candidates
        .or_else(|| chunk2.response.as_ref().and_then(|r| r.candidates.clone()))
        .unwrap();
    assert_eq!(
        candidates2[0].content.as_ref().unwrap().parts[0].text.as_deref(),
        Some("Direct candidate")
    );
}

#[test]
fn test_build_antigravity_request_multi_turn_alternation() {
    let request = ProviderRequest {
        model: "gemini-3.7-flash".to_string(),
        messages: vec![
            ModelMessage {
                role: MessageRole::User,
                content: vec![MessageContent::Text {
                    text: "Turn 1 question".to_string(),
                }],
            },
            ModelMessage {
                role: MessageRole::Assistant,
                content: vec![
                    MessageContent::Text {
                        text: "Let me check".to_string(),
                    },
                    MessageContent::ToolCall {
                        call_id: "call-1".to_string(),
                        tool_id: CapabilityId::new(rho_sdk::capability::CapabilityKind::Tool, "read").unwrap(),
                        arguments: serde_json::json!({"path": "test.txt"}),
                    },
                ],
            },
            ModelMessage {
                role: MessageRole::Tool,
                content: vec![MessageContent::ToolResult {
                    call_id: "call-1".to_string(),
                    content: "file contents".to_string(),
                    is_error: false,
                }],
            },
            ModelMessage {
                role: MessageRole::Assistant,
                content: vec![MessageContent::Text {
                    text: "Turn 1 answer".to_string(),
                }],
            },
            ModelMessage {
                role: MessageRole::User,
                content: vec![MessageContent::Text {
                    text: "Turn 2 question".to_string(),
                }],
            },
        ],
        credential: None,
        max_output_tokens: Some(4096),
        tools: Vec::new(),
    };

    let req = build_antigravity_request(&request, "my-project", "gemini-3.7-flash-high");
    // Verify strict role alternation
    let roles: Vec<&str> = req.request.contents.iter().map(|c| c.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "model", "user", "model", "user"]);

    // Verify tool result uses matching function declaration name ("read")
    let tool_turn = &req.request.contents[2];
    assert_eq!(tool_turn.parts[0].function_response.as_ref().unwrap().name, "read");
}
