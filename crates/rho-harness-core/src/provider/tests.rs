use super::*;

#[test]
fn oauth_providers_list_is_exact() {
    assert_eq!(
        ProviderId::OAUTH_PROVIDERS,
        [
            ProviderId::ChatGpt,
            ProviderId::Copilot,
            ProviderId::Antigravity,
            ProviderId::ClaudeCode,
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
    for excluded in [
        ProviderId::Local,
        ProviderId::ChatGpt,
        ProviderId::Copilot,
        ProviderId::Antigravity,
        ProviderId::ClaudeCode,
    ] {
        assert!(!ProviderId::API_KEY_PROVIDERS.contains(&excluded));
    }
}

#[test]
fn supports_oauth_capabilities() {
    let cases = [
        (ProviderId::OpenRouter, true),
        (ProviderId::ChatGpt, true),
        (ProviderId::Copilot, true),
        (ProviderId::Antigravity, true),
        (ProviderId::ClaudeCode, true),
        (ProviderId::Anthropic, false),
        (ProviderId::OpenAi, false),
        (ProviderId::Gemini, false),
        (ProviderId::Local, false),
    ];
    for (p, expected) in cases {
        assert_eq!(p.supports_oauth(), expected);
    }
}

#[test]
fn supports_api_key_capabilities() {
    let cases = [
        (ProviderId::OpenRouter, true),
        (ProviderId::Anthropic, true),
        (ProviderId::OpenAi, true),
        (ProviderId::DeepSeek, true),
        (ProviderId::Gemini, true),
        (ProviderId::ChatGpt, false),
        (ProviderId::Copilot, false),
        (ProviderId::Antigravity, false),
        (ProviderId::ClaudeCode, false),
        (ProviderId::Local, false),
    ];
    for (p, expected) in cases {
        assert_eq!(p.supports_api_key(), expected);
    }
}

#[test]
fn credential_strategies_and_labels() {
    let cases = [
        (
            ProviderId::OpenRouter,
            CredentialStrategy::OAuthOrApiKey,
            "OAuth or API key",
        ),
        (
            ProviderId::ChatGpt,
            CredentialStrategy::SubscriptionOAuth,
            "subscription OAuth",
        ),
        (
            ProviderId::ClaudeCode,
            CredentialStrategy::SubscriptionOAuth,
            "subscription OAuth",
        ),
        (ProviderId::Anthropic, CredentialStrategy::ApiKey, "API key"),
        (ProviderId::Local, CredentialStrategy::Local, "local; no login"),
    ];
    for (p, strategy, label) in cases {
        assert_eq!((p.credential_strategy(), p.auth_mode_label()), (strategy, label));
    }
}

#[test]
fn api_key_environment_variables() {
    let cases = [
        (ProviderId::OpenRouter, Some("OPENROUTER_API_KEY")),
        (ProviderId::Anthropic, Some("ANTHROPIC_API_KEY")),
        (ProviderId::ChatGpt, None),
        (ProviderId::ClaudeCode, None),
        (ProviderId::Local, None),
    ];
    for (p, expected) in cases {
        assert_eq!(p.api_key_env(), expected);
    }
}

#[test]
fn all_variants_are_unique_and_represented() {
    assert_eq!(ProviderId::ALL.len(), 15);
    for provider in ProviderId::ALL {
        let parsed = ProviderId::from_str(provider.as_str()).expect("canonical string must parse");
        assert_eq!(parsed, provider);
    }
}

#[test]
fn from_str_aliases_and_case_insensitivity() {
    let cases = [
        ("  OPENROUTER  ", ProviderId::OpenRouter),
        ("google-antigravity", ProviderId::Antigravity),
        ("claude", ProviderId::ClaudeCode),
        ("claude-code", ProviderId::ClaudeCode),
        ("claude-oauth", ProviderId::ClaudeCode),
        ("google", ProviderId::Gemini),
        ("ollama", ProviderId::Local),
        ("ollamacloud", ProviderId::OllamaCloud),
    ];
    for (alias, expected) in cases {
        assert_eq!(ProviderId::from_str(alias).unwrap(), expected);
    }
    assert!(ProviderId::from_str("nonexistent-ai").is_err());
}

#[test]
fn infer_provider_for_model_resolves_common_prefixes() {
    let cases = [
        ("claude-3-7-sonnet-20250219", Some("anthropic")),
        ("gpt-4o", Some("openai")),
        ("o3-mini", Some("openai")),
        ("gemini-2.0-flash", Some("gemini")),
        ("deepseek-chat", Some("deepseek")),
        ("grok-2", Some("xai")),
        ("mistral-large-latest", Some("mistral")),
        ("llama-3.3-70b-versatile", Some("groq")),
        ("meta-llama/llama-3-70b", Some("openrouter")),
        ("unknown-model", None),
    ];
    for (model, expected) in cases {
        assert_eq!(infer_provider_for_model(model), expected);
    }
}
