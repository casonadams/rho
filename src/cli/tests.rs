use super::auth::*;

#[test]
fn unknown_login_provider_fails_locally() {
    let error = select_provider(Some("unknown-provider"), "anthropic").unwrap_err();
    assert!(error.to_string().contains("unsupported AI provider"));
}

#[test]
fn test_rpc_request_and_response_roundtrip() {
    use rho_core::rpc::protocol::{RpcCommand, RpcRequest, RpcResponse};

    let prompt_req = RpcRequest {
        id: Some("1".to_string()),
        command: RpcCommand::Prompt {
            message: "Analyze repo".to_string(),
            images: None,
            streaming_behavior: Some("steer".to_string()),
        },
    };
    let json = serde_json::to_string(&prompt_req).unwrap();
    let deserialized: RpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, Some("1".to_string()));
    assert!(matches!(deserialized.command, RpcCommand::Prompt { ref message, .. } if message == "Analyze repo"));

    let resp = RpcResponse::success(Some("1".to_string()), "prompt", None);
    let resp_json = serde_json::to_string(&resp).unwrap();
    assert!(resp_json.contains("\"success\":true"));
}
