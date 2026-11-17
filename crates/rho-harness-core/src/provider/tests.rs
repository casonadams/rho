use super::*;

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
    assert_eq!(ProviderId::ALL.len(), 14);
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
    let assert_provider = |m: &str, p: Option<&str>| {
        assert_eq!(infer_provider_for_model(m), p);
    };
    assert_provider("claude-3-7-sonnet-20250219", Some("anthropic"));
    assert_provider("haiku-5.5", Some("anthropic"));
    assert_provider("haiku-5-5", Some("anthropic"));
    assert_provider("sonnet-5.5", Some("anthropic"));
    assert_provider("opus-5.5", Some("anthropic"));
    assert_provider("gpt-4o", Some("openai"));
    assert_provider("o3-mini", Some("openai"));
    assert_provider("gemini-2.0-flash", Some("gemini"));
    assert_provider("deepseek-chat", Some("deepseek"));
    assert_provider("grok-2", Some("xai"));
    assert_provider("mistral-large-latest", Some("mistral"));
    assert_provider("llama-3.3-70b-versatile", Some("groq"));
    assert_provider("meta-llama/llama-3-70b", Some("openrouter"));
    assert_provider("unknown-model", None);
}

#[test]
fn test_parse_model_spec() {
    let cases = [
        ("anthropic/claude-3-7-sonnet", ("anthropic", "claude-3-7-sonnet")),
        ("openai/gpt-4o", ("openai", "gpt-4o")),
        ("local/qwen2.5-coder:7b", ("local", "qwen2.5-coder:7b")),
        ("ollama/llama3.2:latest", ("ollama", "llama3.2:latest")),
        (
            "openrouter/anthropic/claude-3.7-sonnet",
            ("openrouter", "anthropic/claude-3.7-sonnet"),
        ),
        (
            "openrouter/meta-llama/llama-3.3-70b-instruct:free",
            ("openrouter", "meta-llama/llama-3.3-70b-instruct:free"),
        ),
        ("  anthropic / claude-3-7-sonnet  ", ("anthropic", "claude-3-7-sonnet")),
        ("claude-3-7-sonnet", ("", "claude-3-7-sonnet")),
        ("", ("", "")),
    ];
    for (input, (expected_p, expected_m)) in cases {
        assert_eq!(
            parse_model_spec(input),
            (expected_p.to_string(), expected_m.to_string())
        );
    }
}
