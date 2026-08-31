use super::auth::*;
use super::parse_replacements;
use crate::engine::provider::{CredentialStrategy, ProviderId};

#[test]
fn replacement_flags_are_validated_and_deduplicated() {
    let replacements = parse_replacements(vec!["tool:bash".to_string(), "tool:bash".to_string()]).unwrap();
    assert_eq!(replacements.len(), 1);
    assert!(parse_replacements(vec!["not-a-capability".to_string()]).is_err());
}

#[test]
fn unknown_login_provider_fails_locally() {
    let error = select_provider(Some("unknown-provider"), "anthropic").unwrap_err();
    assert!(error.to_string().contains("unsupported AI provider"));
}

#[test]
fn provider_help_identities_have_distinct_auth_modes() {
    assert_eq!(
        select_provider(Some("openai"), "anthropic")
            .unwrap()
            .credential_strategy(),
        CredentialStrategy::ApiKey
    );
    assert_eq!(
        select_provider(Some("chatgpt"), "anthropic")
            .unwrap()
            .credential_strategy(),
        CredentialStrategy::SubscriptionOAuth
    );
    assert_eq!(
        select_provider(Some("copilot"), "anthropic")
            .unwrap()
            .credential_strategy(),
        CredentialStrategy::SubscriptionOAuth
    );
}

#[tokio::test]
async fn subscription_login_dispatches_chatgpt_and_copilot_without_credentials() {
    for expected in [ProviderId::ChatGpt, ProviderId::Copilot] {
        let selected = std::sync::Arc::new(std::sync::Mutex::new(None));
        let observed = selected.clone();
        login_subscription(
            expected,
            move |provider| {
                *observed.lock().unwrap() = Some(provider);
                std::future::ready(Ok(()))
            },
            std::future::pending(),
        )
        .await
        .unwrap();
        assert_eq!(*selected.lock().unwrap(), Some(expected));
    }
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
