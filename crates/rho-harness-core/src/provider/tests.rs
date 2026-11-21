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
        assert_eq!((p.info().credentials, p.auth_mode_label()), (strategy, label));
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
    let mut names = std::collections::HashSet::new();
    for provider in ProviderId::ALL {
        let info = provider.info();
        for name in std::iter::once(info.name).chain(info.aliases.iter().copied()) {
            assert!(names.insert(name), "{name} names two providers");
            assert_eq!(ProviderId::from_str(name).unwrap(), provider);
        }
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
    assert_provider("unknown-model", None);
}

fn route(connection: &str, model: &str) -> ModelRoute {
    ModelRoute {
        connection: Connection::parse(connection),
        model: model.to_string(),
    }
}

#[test]
fn parse_prefers_prefix_then_explicit_then_inferred_then_fallback() {
    let cases = [
        (
            ("anthropic/claude-3-7-sonnet", None, "local"),
            route("anthropic", "claude-3-7-sonnet"),
        ),
        (
            ("  anthropic / claude-3-7-sonnet  ", None, "local"),
            route("anthropic", "claude-3-7-sonnet"),
        ),
        (
            ("ollama/llama3.2:latest", None, "local"),
            route("local", "llama3.2:latest"),
        ),
        (("claude-code/opus", None, "local"), route("claude", "opus")),
        (("Acme/m-1", None, "local"), route("acme", "m-1")),
        (
            ("openrouter/anthropic/claude-3.7-sonnet", None, "local"),
            route("openrouter", "anthropic/claude-3.7-sonnet"),
        ),
        (
            ("anthropic/claude-x", Some("openai"), "local"),
            route("anthropic", "claude-x"),
        ),
        (("gpt-4o", Some("chatgpt"), "local"), route("chatgpt", "gpt-4o")),
        (("gpt-4o", Some("  "), "claude"), route("openai", "gpt-4o")),
        (
            ("qwen2.5-coder:7b", None, "chatgpt"),
            route("chatgpt", "qwen2.5-coder:7b"),
        ),
        (("/qwen", None, "local"), route("local", "qwen")),
        (("", None, "local"), route("local", "")),
        (
            ("claude:Me@Work.com/opus", None, "local"),
            route("claude:me@work.com", "opus"),
        ),
        (
            ("claude-code:me@work.com/opus", None, "local"),
            route("claude:me@work.com", "opus"),
        ),
        (("claude:/opus", None, "local"), route("claude", "opus")),
        (
            ("opus", None, "claude:me@work.com"),
            route("claude:me@work.com", "opus"),
        ),
        (
            ("local/qwen2.5-coder:7b", None, "claude:me@work.com"),
            route("local", "qwen2.5-coder:7b"),
        ),
        (
            ("gpt-4o", None, "openai:me@work.com"),
            route("openai:me@work.com", "gpt-4o"),
        ),
        (("gpt-4o", None, "claude:me@work.com"), route("openai", "gpt-4o")),
    ];
    for ((spec, explicit, fallback), expected) in cases {
        assert_eq!(ModelRoute::parse(spec, explicit, fallback), expected, "{spec}");
    }
}

#[test]
fn new_keeps_slashed_ids_and_drops_only_a_matching_prefix() {
    let cases = [
        (
            ("openrouter", "anthropic/claude-3.7-sonnet"),
            "openrouter/anthropic/claude-3.7-sonnet",
        ),
        (
            ("openrouter", "openrouter/anthropic/claude-3.7-sonnet"),
            "openrouter/anthropic/claude-3.7-sonnet",
        ),
        (("chatgpt", "chatgpt/gpt-6-sol"), "chatgpt/gpt-6-sol"),
        (("claude-code", "claude/opus"), "claude/opus"),
        (("ollama", " qwen "), "local/qwen"),
        (("claude:me@work.com", "opus"), "claude:me@work.com/opus"),
        (
            ("claude:me@work.com", "claude:me@work.com/opus"),
            "claude:me@work.com/opus",
        ),
        (("claude:me@work.com", "claude/opus"), "claude:me@work.com/claude/opus"),
    ];
    for ((provider, model), expected) in cases {
        assert_eq!(ModelRoute::new(provider, model).to_string(), expected);
    }
}

#[test]
fn connection_is_the_auth_key_of_one_login() {
    let cases = [
        ("claude", ("claude", None), "claude"),
        ("claude-code", ("claude", None), "claude"),
        (
            "claude:Me@Work.com",
            ("claude", Some("me@work.com")),
            "claude:me@work.com",
        ),
        (" chatgpt: ", ("chatgpt", None), "chatgpt"),
        ("acme:team", ("acme", Some("team")), "acme:team"),
    ];
    for (input, (provider, account), key) in cases {
        let connection = Connection::parse(input);
        assert_eq!(
            (connection.provider.as_str(), connection.account.as_deref()),
            (provider, account),
            "{input}"
        );
        assert_eq!(connection.to_string(), key);
        assert_eq!(ModelRoute::new(input, "m").connection, connection);
    }
    assert_eq!(
        Connection::account(ProviderId::ClaudeCode, "Me@Work.com").to_string(),
        "claude:me@work.com"
    );
    assert_eq!(Connection::parse("acme").provider_id(), None);
}
