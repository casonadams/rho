use super::*;
use rho_core::provider::CredentialStrategy;
use rho_sdk::contract::{AuthenticationMethod, ProviderCapability};

#[test]
fn provider_parity_matrix_covers_catalog_auth_context_quota_and_status() {
    let expected = [
        (ProviderId::Anthropic, CredentialStrategy::ApiKey, false),
        (ProviderId::OpenAi, CredentialStrategy::ApiKey, false),
        (ProviderId::ChatGpt, CredentialStrategy::SubscriptionOAuth, true),
        (ProviderId::Copilot, CredentialStrategy::SubscriptionOAuth, false),
        (ProviderId::Antigravity, CredentialStrategy::SubscriptionOAuth, true),
        (ProviderId::DeepSeek, CredentialStrategy::ApiKey, false),
        (ProviderId::Gemini, CredentialStrategy::ApiKey, false),
        (ProviderId::Groq, CredentialStrategy::ApiKey, false),
        (ProviderId::Ollama, CredentialStrategy::Local, false),
        (ProviderId::OpenRouter, CredentialStrategy::ApiKey, false),
        (ProviderId::XAi, CredentialStrategy::ApiKey, false),
        (ProviderId::Mistral, CredentialStrategy::ApiKey, false),
        (ProviderId::Cohere, CredentialStrategy::ApiKey, false),
    ];
    for (id, strategy, quota) in expected {
        let provider = BuiltinProvider::new(id);
        let descriptor = provider.descriptor();
        assert_eq!(descriptor.id.name(), id.as_str());
        assert_eq!(id.credential_strategy(), strategy);
        assert_eq!(ActiveProvider::Builtin(provider).credential_strategy(), strategy);
        assert_eq!(provider.facts().supports_quota, quota);
        assert!(provider.facts().supports_status);
        assert_eq!(descriptor.models.len(), provider.model_catalog().models().len());
        for model in descriptor.models {
            assert_eq!(model.context_limit, context_limit(&model.id).map(|limit| limit as u64));
        }
    }
}

#[test]
fn local_provider_declares_no_auth_and_unknown_provider_fails() {
    let registry = ProviderRegistry::builtins();
    assert_eq!(
        registry.get("ollama").unwrap().descriptor().authentication,
        vec![AuthenticationMethod::None]
    );
    assert!(registry.get("unknown").is_err());
}
