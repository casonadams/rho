use super::*;

#[test]
fn oauth_providers_list_is_exact() {
    assert_eq!(
        ProviderId::OAUTH_PROVIDERS,
        [
            ProviderId::ChatGpt,
            ProviderId::Copilot,
            ProviderId::Antigravity,
            ProviderId::OpenRouter,
        ]
    );
}

#[test]
fn api_key_providers_list_excludes_local_and_pure_oauth() {
    assert_eq!(
        ProviderId::API_KEY_PROVIDERS,
        [
            ProviderId::Anthropic,
            ProviderId::OpenAi,
            ProviderId::DeepSeek,
            ProviderId::Gemini,
            ProviderId::Groq,
            ProviderId::OllamaCloud,
            ProviderId::OpenRouter,
            ProviderId::XAi,
            ProviderId::Mistral,
            ProviderId::Cohere,
        ]
    );
    assert!(!ProviderId::API_KEY_PROVIDERS.contains(&ProviderId::Local));
    assert!(!ProviderId::API_KEY_PROVIDERS.contains(&ProviderId::ChatGpt));
    assert!(!ProviderId::API_KEY_PROVIDERS.contains(&ProviderId::Copilot));
    assert!(!ProviderId::API_KEY_PROVIDERS.contains(&ProviderId::Antigravity));
}

#[test]
fn supports_oauth_and_api_key_capabilities() {
    // OpenRouter supports both
    assert!(ProviderId::OpenRouter.supports_oauth());
    assert!(ProviderId::OpenRouter.supports_api_key());

    // Pure OAuth providers
    for provider in [ProviderId::ChatGpt, ProviderId::Copilot, ProviderId::Antigravity] {
        assert!(provider.supports_oauth(), "{provider} should support OAuth");
        assert!(!provider.supports_api_key(), "{provider} should not support API key");
    }

    // Pure API key providers
    for provider in [
        ProviderId::Anthropic,
        ProviderId::OpenAi,
        ProviderId::DeepSeek,
        ProviderId::Gemini,
        ProviderId::Groq,
        ProviderId::OllamaCloud,
        ProviderId::XAi,
        ProviderId::Mistral,
        ProviderId::Cohere,
    ] {
        assert!(!provider.supports_oauth(), "{provider} should not support OAuth");
        assert!(provider.supports_api_key(), "{provider} should support API key");
    }

    // Local requires no credentials
    assert!(!ProviderId::Local.supports_oauth());
    assert!(!ProviderId::Local.supports_api_key());
}

#[test]
fn credential_strategies_and_labels() {
    assert_eq!(
        ProviderId::OpenRouter.credential_strategy(),
        CredentialStrategy::OAuthOrApiKey
    );
    assert_eq!(ProviderId::OpenRouter.auth_mode_label(), "OAuth or API key");

    assert_eq!(
        ProviderId::ChatGpt.credential_strategy(),
        CredentialStrategy::SubscriptionOAuth
    );
    assert_eq!(ProviderId::ChatGpt.auth_mode_label(), "subscription OAuth");

    assert_eq!(ProviderId::Anthropic.credential_strategy(), CredentialStrategy::ApiKey);
    assert_eq!(ProviderId::Anthropic.auth_mode_label(), "API key");

    assert_eq!(ProviderId::Local.credential_strategy(), CredentialStrategy::Local);
    assert_eq!(ProviderId::Local.auth_mode_label(), "local; no login");
}

#[test]
fn api_key_environment_variables() {
    assert_eq!(ProviderId::OpenRouter.api_key_env(), Some("OPENROUTER_API_KEY"));
    assert_eq!(ProviderId::Anthropic.api_key_env(), Some("ANTHROPIC_API_KEY"));
    assert_eq!(ProviderId::ChatGpt.api_key_env(), None);
    assert_eq!(ProviderId::Local.api_key_env(), None);
}

#[test]
fn all_variants_are_unique_and_represented() {
    assert_eq!(ProviderId::ALL.len(), 14);
    for provider in ProviderId::ALL {
        let parsed = ProviderId::from_str(provider.as_str()).expect("canonical string must parse");
        assert_eq!(parsed, provider);
    }
}

#[test]
fn from_str_aliases_and_case_insensitivity() {
    assert_eq!(ProviderId::from_str("  OPENROUTER  ").unwrap(), ProviderId::OpenRouter);
    assert_eq!(
        ProviderId::from_str("google-antigravity").unwrap(),
        ProviderId::Antigravity
    );
    assert_eq!(ProviderId::from_str("google").unwrap(), ProviderId::Gemini);
    assert_eq!(ProviderId::from_str("ollama").unwrap(), ProviderId::Local);
    assert_eq!(ProviderId::from_str("ollamacloud").unwrap(), ProviderId::OllamaCloud);
    assert!(ProviderId::from_str("nonexistent-ai").is_err());
}
