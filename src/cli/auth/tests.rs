use super::provider::{AuthMethod, api_key_provider_options, oauth_provider_options, resolve_provider_name};
use crate::config::Config;

#[test]
fn built_in_names_canonicalize_and_custom_names_are_kept() {
    assert_eq!(resolve_provider_name(Some("Google"), "anthropic"), "gemini");
    assert_eq!(
        resolve_provider_name(Some("google-antigravity"), "anthropic"),
        "antigravity"
    );
    assert_eq!(resolve_provider_name(None, "GROQ"), "groq");
    assert_eq!(resolve_provider_name(Some("acme"), "anthropic"), "acme");
    assert_eq!(resolve_provider_name(Some("Acme Cloud"), "anthropic"), "acme cloud");
}

#[test]
fn oauth_provider_options_are_filtered_and_concise() {
    let options = oauth_provider_options();

    let ids: Vec<&str> = options.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, vec!["antigravity", "chatgpt", "copilot", "openrouter"]);
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids, "OAuth provider options must be sorted alphabetically");

    for (id, desc) in &options {
        assert!(
            desc.len() <= 40,
            "Description for {id} is too long ({len} chars): {desc}",
            len = desc.len()
        );
        assert!(
            !desc.contains("3.5") && !desc.contains("4o") && !desc.contains("2.0"),
            "Description for {id} contains version numbers: {desc}"
        );
    }
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

    assert!(
        !options.iter().any(|(id, _)| id == "local"),
        "local should not appear in API key options"
    );

    assert!(!options.iter().any(|(id, _)| id == "chatgpt"));
    assert!(!options.iter().any(|(id, _)| id == "copilot"));
    assert!(!options.iter().any(|(id, _)| id == "antigravity"));

    assert!(options.iter().any(|(id, _)| id == "openrouter"));
    assert!(options.iter().any(|(id, _)| id == "custom-llm"));

    let ids: Vec<&str> = options.iter().map(|(id, _)| id.as_str()).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(
        ids, sorted_ids,
        "API key provider options must be sorted alphabetically"
    );

    for (id, desc) in &options {
        assert!(
            desc.len() <= 40,
            "Description for {id} is too long ({len} chars): {desc}",
            len = desc.len()
        );
        assert!(
            !desc.contains("3.5") && !desc.contains("4o") && !desc.contains("2.0"),
            "Description for {id} contains version numbers: {desc}"
        );
    }
}

#[test]
fn auth_method_equality() {
    assert_eq!(AuthMethod::OAuth, AuthMethod::OAuth);
    assert_eq!(AuthMethod::ApiKey, AuthMethod::ApiKey);
    assert_ne!(AuthMethod::OAuth, AuthMethod::ApiKey);
}
