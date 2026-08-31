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
    assert_eq!(req.request_type, "AGENT");
    assert_eq!(req.user_agent, "ANTIGRAVITY");
    assert!(req.request.system_instruction.is_some());
    assert_eq!(req.request.contents.len(), 1);
    assert_eq!(req.request.contents[0].role, "user");
    assert_eq!(req.request.tools.as_ref().unwrap().len(), 1);
    assert_eq!(
        req.request.tools.as_ref().unwrap()[0].function_declarations[0].name,
        "read"
    );
}
