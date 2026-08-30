use super::*;
use crate::auth::{ApiKeyVerifier, AuthStore, Credential, VerificationStatus};
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

fn request<'a>(model: &'a str, config_dir: &'a Path) -> ModelRequest<'a> {
    ModelRequest { model, config_dir }
}

fn auth_store() -> AuthStore {
    let credentials = ProviderId::API_KEY_PROVIDERS
        .into_iter()
        .filter(|provider| provider.credential_strategy() == CredentialStrategy::ApiKey)
        .map(|provider| {
            (
                provider.as_str().to_string(),
                Credential::ApiKey {
                    key: "fixture-key-not-secret".to_string(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut auth_store = AuthStore::default();
    auth_store.credentials = credentials;
    auth_store
}

#[test]
fn parses_provider_identities_and_only_documented_alias() {
    let cases = [
        ("anthropic", ProviderId::Anthropic),
        ("OPENAI", ProviderId::OpenAi),
        ("chatgpt", ProviderId::ChatGpt),
        ("copilot", ProviderId::Copilot),
        ("deepseek", ProviderId::DeepSeek),
        ("gemini", ProviderId::Gemini),
        ("google", ProviderId::Gemini),
        ("groq", ProviderId::Groq),
        ("ollama", ProviderId::Ollama),
        ("openrouter", ProviderId::OpenRouter),
        ("xai", ProviderId::XAi),
        ("mistral", ProviderId::Mistral),
        ("cohere", ProviderId::Cohere),
    ];
    for (input, expected) in cases {
        assert_eq!(ProviderId::from_str(input).unwrap(), expected);
    }
    assert!(ProviderId::from_str("open-ai").is_err());
}

#[test]
fn keeps_openai_chatgpt_and_copilot_distinct() {
    assert_eq!(ProviderId::OpenAi.credential_strategy(), CredentialStrategy::ApiKey);
    assert_eq!(
        ProviderId::ChatGpt.credential_strategy(),
        CredentialStrategy::SubscriptionOAuth
    );
    assert_eq!(
        ProviderId::Copilot.credential_strategy(),
        CredentialStrategy::SubscriptionOAuth
    );
    assert_ne!(ProviderId::OpenAi, ProviderId::ChatGpt);
    assert_ne!(ProviderId::ChatGpt, ProviderId::Copilot);
}

#[test]
fn constructs_every_provider_model_without_network_or_device_flow() {
    let auth = auth_store();
    let config_dir = std::env::temp_dir().join("rho-provider-construction");
    for provider in ProviderId::ALL {
        let handle = ProviderFactory::create_model_for(provider, request("fixture-model", &config_dir), &auth).unwrap();
        assert_eq!(handle.label(), Some(provider.as_str()));
    }
}

#[test]
fn unknown_provider_fails_before_auth_lookup() {
    let error = ProviderFactory::create_model("unknown", request("model", Path::new("unused")), &AuthStore::default())
        .unwrap_err();
    assert!(error.to_string().contains("unsupported AI provider"));
}

#[test]
fn missing_key_names_variable_without_credentials() {
    let error = ProviderFactory::create_model_for(
        ProviderId::Anthropic,
        request("model", Path::new("unused")),
        &AuthStore::default(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Auth error: ANTHROPIC_API_KEY is not set");
}

#[test]
fn subscription_models_use_their_own_rig_identity() {
    let auth = auth_store();
    let root = Path::new("oauth-fixture-root");
    let chatgpt = ProviderFactory::create_model_for(ProviderId::ChatGpt, request("model", root), &auth).unwrap();
    let copilot = ProviderFactory::create_model_for(ProviderId::Copilot, request("model", root), &auth).unwrap();
    assert_eq!(chatgpt.label(), Some("chatgpt"));
    assert_eq!(copilot.label(), Some("copilot"));
}

#[tokio::test]
async fn unsupported_verification_is_explicit() {
    let status = RigCredentialVerifier
        .verify(ProviderId::Ollama, "unused")
        .await
        .unwrap();
    assert_eq!(status, VerificationStatus::Deferred);
}

#[test]
fn curated_listing_is_labeled_as_not_live() {
    let catalog = curated(ProviderId::ChatGpt);
    assert!(catalog.source_label().contains("not live discovery"));
    assert!(catalog.models().contains(&"gpt-5.3-codex"));
}

#[test]
fn supported_listing_capabilities_are_rig_backed() {
    fn lists<T: rig::client::ModelListingClient>() {}
    lists::<rig::providers::anthropic::Client>();
    lists::<rig::providers::openai::Client>();
    lists::<rig::providers::deepseek::Client>();
    lists::<rig::providers::gemini::Client>();
    lists::<rig::providers::groq::Client>();
    lists::<rig::providers::ollama::Client>();
    lists::<rig::providers::openrouter::Client>();
    lists::<rig::providers::mistral::Client>();
    lists::<rig::providers::copilot::Client>();
}

#[test]
fn model_debug_output_does_not_expose_key() {
    let sentinel = "credential-secret-sentinel";
    let mut auth = AuthStore::default();
    auth.credentials.insert(
        "openai".to_string(),
        Credential::ApiKey {
            key: sentinel.to_string(),
        },
    );
    let handle =
        ProviderFactory::create_model_for(ProviderId::OpenAi, request("model", Path::new("unused")), &auth).unwrap();
    assert!(!format!("{handle:?}").contains(sentinel));
}
