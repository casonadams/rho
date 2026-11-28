use super::*;
use test_case::test_case;

#[test_case(ProviderId::OpenRouter, CredentialStrategy::OAuthOrApiKey, "OAuth or API key"; "openrouter")]
#[test_case(ProviderId::ChatGpt, CredentialStrategy::SubscriptionOAuth, "subscription OAuth"; "chatgpt")]
#[test_case(ProviderId::ClaudeCode, CredentialStrategy::SubscriptionOAuth, "subscription OAuth"; "claude code")]
#[test_case(ProviderId::Anthropic, CredentialStrategy::ApiKey, "API key"; "anthropic")]
#[test_case(ProviderId::Local, CredentialStrategy::Local, "local; no login"; "local")]
fn credential_strategies_and_labels(p: ProviderId, strategy: CredentialStrategy, label: &str) {
    assert_eq!((p.info().credentials, p.auth_mode_label()), (strategy, label));
}

#[test_case(ProviderId::OpenRouter, Some("OPENROUTER_API_KEY"); "openrouter")]
#[test_case(ProviderId::Anthropic, Some("ANTHROPIC_API_KEY"); "anthropic")]
#[test_case(ProviderId::ChatGpt, None; "chatgpt")]
#[test_case(ProviderId::ClaudeCode, None; "claude code")]
#[test_case(ProviderId::Local, None; "local")]
fn api_key_environment_variables(p: ProviderId, expected: Option<&str>) {
    assert_eq!(p.api_key_env(), expected);
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

#[test_case("  OPENROUTER  ", ProviderId::OpenRouter; "padded uppercase openrouter")]
#[test_case("google-antigravity", ProviderId::Antigravity; "google antigravity")]
#[test_case("claude", ProviderId::ClaudeCode; "claude")]
#[test_case("claude-code", ProviderId::ClaudeCode; "claude code")]
#[test_case("claude-oauth", ProviderId::ClaudeCode; "claude oauth")]
#[test_case("google", ProviderId::Gemini; "google")]
#[test_case("ollama", ProviderId::Local; "ollama")]
#[test_case("ollamacloud", ProviderId::OllamaCloud; "ollamacloud")]
fn from_str_aliases_and_case_insensitivity(alias: &str, expected: ProviderId) {
    assert_eq!(ProviderId::from_str(alias).unwrap(), expected);
}

#[test]
fn from_str_rejects_unknown_providers() {
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

#[test_case("anthropic/claude-3-7-sonnet", None, "local", route("anthropic", "claude-3-7-sonnet"); "provider prefix")]
#[test_case("  anthropic / claude-3-7-sonnet  ", None, "local", route("anthropic", "claude-3-7-sonnet"); "padded provider prefix")]
#[test_case("ollama/llama3.2:latest", None, "local", route("local", "llama3.2:latest"); "ollama alias prefix")]
#[test_case("claude-code/opus", None, "local", route("claude", "opus"); "claude code alias prefix")]
#[test_case("Acme/m-1", None, "local", route("acme", "m-1"); "custom provider prefix")]
#[test_case("openrouter/anthropic/claude-3.7-sonnet", None, "local", route("openrouter", "anthropic/claude-3.7-sonnet"); "slashed model id")]
#[test_case("anthropic/claude-x", Some("openai"), "local", route("anthropic", "claude-x"); "prefix beats explicit")]
#[test_case("gpt-4o", Some("chatgpt"), "local", route("chatgpt", "gpt-4o"); "explicit beats inferred")]
#[test_case("gpt-4o", Some("  "), "claude", route("openai", "gpt-4o"); "blank explicit falls to inferred")]
#[test_case("qwen2.5-coder:7b", None, "chatgpt", route("chatgpt", "qwen2.5-coder:7b"); "uninferrable uses fallback")]
#[test_case("/qwen", None, "local", route("local", "qwen"); "empty prefix uses fallback")]
#[test_case("", None, "local", route("local", ""); "empty spec")]
#[test_case("claude:Me@Work.com/opus", None, "local", route("claude:me@work.com", "opus"); "account prefix lowercased")]
#[test_case("claude-code:me@work.com/opus", None, "local", route("claude:me@work.com", "opus"); "alias account prefix")]
#[test_case("claude:/opus", None, "local", route("claude", "opus"); "empty account prefix")]
#[test_case("opus", None, "claude:me@work.com", route("claude:me@work.com", "opus"); "account fallback")]
#[test_case("local/qwen2.5-coder:7b", None, "claude:me@work.com", route("local", "qwen2.5-coder:7b"); "prefix beats account fallback")]
#[test_case("gpt-4o", None, "openai:me@work.com", route("openai:me@work.com", "gpt-4o"); "inferred provider keeps matching account")]
#[test_case("gpt-4o", None, "claude:me@work.com", route("openai", "gpt-4o"); "inferred provider drops other account")]
fn parse_prefers_prefix_then_explicit_then_inferred_then_fallback(
    spec: &str,
    explicit: Option<&str>,
    fallback: &str,
    expected: ModelRoute,
) {
    assert_eq!(ModelRoute::parse(spec, explicit, fallback), expected);
}

#[test_case("openrouter", "anthropic/claude-3.7-sonnet", "openrouter/anthropic/claude-3.7-sonnet"; "slashed id kept")]
#[test_case("openrouter", "openrouter/anthropic/claude-3.7-sonnet", "openrouter/anthropic/claude-3.7-sonnet"; "matching prefix dropped")]
#[test_case("chatgpt", "chatgpt/gpt-6-sol", "chatgpt/gpt-6-sol"; "chatgpt prefix dropped")]
#[test_case("claude-code", "claude/opus", "claude/opus"; "alias provider prefix dropped")]
#[test_case("ollama", " qwen ", "local/qwen"; "padded model on alias provider")]
#[test_case("claude:me@work.com", "opus", "claude:me@work.com/opus"; "account connection")]
#[test_case("claude:me@work.com", "claude:me@work.com/opus", "claude:me@work.com/opus"; "matching account prefix dropped")]
#[test_case("claude:me@work.com", "claude/opus", "claude:me@work.com/claude/opus"; "non matching prefix kept")]
fn new_keeps_slashed_ids_and_drops_only_a_matching_prefix(provider: &str, model: &str, expected: &str) {
    assert_eq!(ModelRoute::new(provider, model).to_string(), expected);
}

#[test_case("claude", "claude", None, "claude"; "bare provider")]
#[test_case("claude-code", "claude", None, "claude"; "alias provider")]
#[test_case("claude:Me@Work.com", "claude", Some("me@work.com"), "claude:me@work.com"; "account lowercased")]
#[test_case(" chatgpt: ", "chatgpt", None, "chatgpt"; "padded empty account")]
#[test_case("acme:team", "acme", Some("team"), "acme:team"; "custom provider account")]
fn connection_is_the_auth_key_of_one_login(input: &str, provider: &str, account: Option<&str>, key: &str) {
    let connection = Connection::parse(input);
    assert_eq!(
        (connection.provider.as_str(), connection.account.as_deref()),
        (provider, account)
    );
    assert_eq!(connection.to_string(), key);
    assert_eq!(ModelRoute::new(input, "m").connection, connection);
}

#[test]
fn connection_accounts_normalize_and_custom_providers_have_no_id() {
    assert_eq!(
        Connection::account(ProviderId::ClaudeCode, "Me@Work.com").to_string(),
        "claude:me@work.com"
    );
    assert_eq!(Connection::parse("acme").provider_id(), None);
}
