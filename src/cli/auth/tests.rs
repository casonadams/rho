use super::provider::{AuthMethod, api_key_provider_options, oauth_provider_options, resolve_provider_name};
use crate::config::Config;

#[test]
fn built_in_names_canonicalize_and_custom_names_are_kept() {
    let cases = [
        (Some("Google"), "anthropic", "gemini"),
        (Some("google-antigravity"), "anthropic", "antigravity"),
        (None, "GROQ", "groq"),
        (Some("acme"), "anthropic", "acme"),
        (Some("Acme Cloud"), "anthropic", "acme cloud"),
    ];
    for (input, default, expected) in cases {
        assert_eq!(resolve_provider_name(input, default), expected);
    }
}

fn assert_descriptions_valid(options: &[(&str, &str)]) {
    for (id, desc) in options {
        assert!(desc.len() <= 40, "{id} desc too long: {desc}");
        assert!(
            !desc.contains("3.5") && !desc.contains("4o") && !desc.contains("2.0"),
            "{id} contains version: {desc}"
        );
    }
}

#[test]
fn oauth_provider_options_are_filtered_and_concise() {
    let options = oauth_provider_options();
    let ids: Vec<&str> = options.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, vec!["antigravity", "chatgpt", "claude", "copilot", "openrouter"]);
    assert_descriptions_valid(&options);
}

#[test]
fn api_key_provider_options_are_filtered_and_concise() {
    let mut config = Config::default();
    config.providers.insert(
        "custom-llm".to_string(),
        rho_harness_core::config::ProviderConfig {
            base_url: "http://localhost:8000".to_string(),
            key_env: None,
            default_model: None,
        },
    );

    let options = api_key_provider_options(&config);
    for excluded in ["local", "chatgpt", "copilot", "antigravity", "claude"] {
        assert!(!options.iter().any(|(id, _)| id == excluded));
    }
    assert!(options.iter().any(|(id, _)| id == "openrouter") && options.iter().any(|(id, _)| id == "custom-llm"));

    let ids: Vec<&str> = options.iter().map(|(id, _)| id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn auth_method_equality() {
    assert_eq!(AuthMethod::OAuth, AuthMethod::OAuth);
    assert_eq!(AuthMethod::ApiKey, AuthMethod::ApiKey);
    assert_ne!(AuthMethod::OAuth, AuthMethod::ApiKey);
}

#[test]
fn read_key_from_reader_reads_clean_lines_and_crlf() {
    let unix_input = b"sk-my-api-key-123\n";
    let key = super::read_key_from_reader(&unix_input[..]).unwrap();
    assert_eq!(key, "sk-my-api-key-123");

    let win_input = b"sk-my-api-key-456\r\n";
    let key = super::read_key_from_reader(&win_input[..]).unwrap();
    assert_eq!(key, "sk-my-api-key-456");
}

#[test]
fn read_key_from_reader_empty_or_whitespace_fails() {
    let empty_input = b"";
    let err = super::read_key_from_reader(&empty_input[..]).unwrap_err();
    assert!(err.to_string().contains("No API key provided on stdin"));

    let whitespace_input = b"   \r\n";
    let err = super::read_key_from_reader(&whitespace_input[..]).unwrap_err();
    assert!(err.to_string().contains("No API key provided on stdin"));
}

#[tokio::test]
async fn login_provider_key_stdin_without_provider_fails() {
    let config = Config::default();
    let temp_auth = tempfile::NamedTempFile::new().unwrap();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();
    let err = super::login_provider(None, true, &config, &mut auth_store)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Provider name required when using --key-stdin")
    );
}

#[test]
fn test_provider_login_name_mappings() {
    use rho_harness_core::provider::ProviderId;

    assert_eq!(super::provider_login_name(ProviderId::ChatGpt), "ChatGPT");
    assert_eq!(super::provider_login_name(ProviderId::Copilot), "GitHub Copilot");
    assert_eq!(
        super::provider_login_name(ProviderId::Antigravity),
        "Google Antigravity"
    );
    assert_eq!(
        super::provider_login_name(ProviderId::ClaudeCode),
        "Claude (Subscription)"
    );
    assert_eq!(super::provider_login_name(ProviderId::OpenRouter), "OpenRouter");
    assert_eq!(super::provider_login_name(ProviderId::OpenAi), "openai");
    assert_eq!(super::provider_login_name(ProviderId::Local), "local");
}

#[test]
fn test_is_default_oauth_provider_checks() {
    use rho_harness_core::provider::ProviderId;

    assert!(super::is_default_oauth_provider(ProviderId::ChatGpt));
    assert!(super::is_default_oauth_provider(ProviderId::Copilot));
    assert!(super::is_default_oauth_provider(ProviderId::Antigravity));
    assert!(super::is_default_oauth_provider(ProviderId::ClaudeCode));
    assert!(!super::is_default_oauth_provider(ProviderId::OpenAi));
    assert!(!super::is_default_oauth_provider(ProviderId::OpenRouter));
    assert!(!super::is_default_oauth_provider(ProviderId::Groq));
}

#[tokio::test]
async fn test_store_api_key_valid_and_empty() {
    let config = Config::default();
    let temp_auth = tempfile::NamedTempFile::new().unwrap();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    let err = super::store_api_key("gemini", "  ", &config, &mut auth_store).unwrap_err();
    assert!(err.to_string().contains("API key cannot be empty"));

    super::store_api_key("gemini", "test-key-123", &config, &mut auth_store).unwrap();
    assert_eq!(
        auth_store.get_key_sync("gemini").unwrap().as_deref(),
        Some("test-key-123")
    );
}

#[test]
fn test_parse_logout_choice() {
    let configured = vec!["anthropic".to_string(), "gemini".to_string()];
    assert_eq!(super::parse_logout_choice("1", &configured).unwrap(), "anthropic");
    assert_eq!(super::parse_logout_choice("2", &configured).unwrap(), "gemini");
    assert!(super::parse_logout_choice("0", &configured).is_err());
    assert!(super::parse_logout_choice("3", &configured).is_err());
    assert!(super::parse_logout_choice("abc", &configured).is_err());
}

#[test]
fn test_prompt_logout_selection() {
    use std::io::Cursor;

    let configured = vec!["anthropic".to_string(), "gemini".to_string()];
    let mut reader = Cursor::new(b"2\n");
    let mut writer = Vec::new();
    let chosen = super::prompt_logout_selection(&mut reader, &mut writer, &configured).unwrap();
    assert_eq!(chosen, "gemini");
    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Select provider credentials to remove:"));
    assert!(output.contains("1. anthropic"));
    assert!(output.contains("2. gemini"));
}

#[test]
fn test_logout_provider_named_and_empty_list() {
    let config = Config::default();
    let temp_auth = tempfile::NamedTempFile::new().unwrap();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    assert!(super::logout_provider(None, &config, &mut auth_store).is_ok());

    auth_store.set_key("gemini", "key-val").unwrap();
    assert!(auth_store.get_key_sync("gemini").unwrap().is_some());
    assert!(super::logout_provider(Some("gemini"), &config, &mut auth_store).is_ok());
    assert!(auth_store.get_key_sync("gemini").unwrap().is_none());
}

#[test]
fn test_should_default_oauth_mappings() {
    use rho_harness_core::provider::ProviderId;

    assert!(super::should_default_oauth(ProviderId::ChatGpt).unwrap());
    assert!(super::should_default_oauth(ProviderId::Copilot).unwrap());
    assert!(super::should_default_oauth(ProviderId::Antigravity).unwrap());
    assert!(super::should_default_oauth(ProviderId::ClaudeCode).unwrap());
    assert!(!super::should_default_oauth(ProviderId::OpenAi).unwrap());
    assert!(!super::should_default_oauth(ProviderId::Groq).unwrap());
    assert!(!super::should_default_oauth(ProviderId::Local).unwrap());
}

#[tokio::test]
async fn test_try_oauth_login_skips_when_api_key_or_unsupported() {
    use rho_harness_core::provider::ProviderId;

    let config = Config::default();
    let temp_auth = tempfile::NamedTempFile::new().unwrap();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    let res1 = super::try_oauth_login(
        ProviderId::OpenAi,
        Some(super::AuthMethod::ApiKey),
        &config,
        &mut auth_store,
    )
    .await
    .unwrap();
    assert!(!res1);

    let res2 = super::try_oauth_login(ProviderId::OpenAi, None, &config, &mut auth_store)
        .await
        .unwrap();
    assert!(!res2);
}

#[test]
fn test_parse_selection_choice() {
    use super::provider::parse_selection_choice;

    assert_eq!(parse_selection_choice("1", 3).unwrap(), 0);
    assert_eq!(parse_selection_choice("3", 3).unwrap(), 2);
    assert!(parse_selection_choice("0", 3).is_err());
    assert!(parse_selection_choice("4", 3).is_err());
    assert!(parse_selection_choice("abc", 3).is_err());
}

#[test]
fn test_prompt_select_from() {
    use super::provider::prompt_select_from;
    use std::io::Cursor;

    let items = vec!["Option A".to_string(), "Option B".to_string()];
    let mut reader = Cursor::new(b"2\n");
    let mut writer = Vec::new();
    let idx = prompt_select_from(&mut reader, &mut writer, "Pick:", &items).unwrap();
    assert_eq!(idx, 1);
    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Pick:"));
    assert!(output.contains("1. Option A"));
    assert!(output.contains("2. Option B"));
}
