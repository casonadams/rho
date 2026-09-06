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
