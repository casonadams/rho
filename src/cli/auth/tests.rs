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
