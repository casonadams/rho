use super::DiscoveredModel;
use super::antigravity::{antigravity_display_name, model_recency_key, sort_models_newest_first};
use super::presets::format_context_tokens;

#[test]
fn test_model_recency_key_parsing() {
    let cases = [
        ("gemini-3.8-flash", (3, 8)),
        ("claude-sonnet-4-6", (4, 6)),
        ("gpt-5", (5, 0)),
        ("model-without-numbers", (0, 0)),
        ("claude-3-7-sonnet-20250219", (3, 7)),
    ];
    for (model, expected) in cases {
        assert_eq!(model_recency_key(model), expected);
    }
}

#[test]
fn test_sort_models_newest_first_descending_and_stable() {
    let models = vec![
        DiscoveredModel::new("gemini-2.0-flash", "Gemini 2.0", ""),
        DiscoveredModel::new("gemini-4-argon", "Gemini 4 Argon", ""),
        DiscoveredModel::new("gemini-3.8-flash", "Gemini 3.8", ""),
        DiscoveredModel::new("gemini-2.0-pro", "Gemini 2.0 Pro", ""),
    ];

    let sorted = sort_models_newest_first(models);
    let ids: Vec<&str> = sorted.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "gemini-4-argon",
            "gemini-3.8-flash",
            "gemini-2.0-flash",
            "gemini-2.0-pro"
        ]
    );
}

#[test]
fn test_antigravity_display_name_token_conversion() {
    let cases = [
        ("gemini-4-carbon", "Gemini 4 Carbon"),
        ("gemini-4-neon", "Gemini 4 Neon"),
        ("gemini-4-hydrogen", "Gemini 4 Hydrogen"),
        ("gemini-4-argon", "Gemini 4 Argon"),
        ("gemini-4-argon-cyber-permissive", "Gemini 4 Argon Cyber Permissive"),
        ("gemini-3.8-flash", "Gemini 3.8 Flash"),
        ("claude-opus-4-6", "Claude Opus 4.6"),
        ("gpt-oss-120b", "GPT OSS 120b"),
    ];
    for (id, expected) in cases {
        assert_eq!(antigravity_display_name(id), expected);
    }
}

#[test]
fn test_format_context_tokens_megabytes_and_kilobytes() {
    assert_eq!(format_context_tokens(1_000_000), "1M ctx");
    assert_eq!(format_context_tokens(2_000_000), "2M ctx");
    assert_eq!(format_context_tokens(200_000), "200k ctx");
    assert_eq!(format_context_tokens(128_000), "128k ctx");
    assert_eq!(format_context_tokens(262_144), "256k ctx");
    assert_eq!(format_context_tokens(131_072), "128k ctx");
    assert_eq!(format_context_tokens(65_536), "64k ctx");
    assert_eq!(format_context_tokens(1_048_576), "1M ctx");
}

#[tokio::test]
async fn discovery_without_credentials_fails_without_network() {
    use rho_harness_core::provider::ProviderId;
    let (_dir, auth) = crate::test_support::temp_auth_store();
    let config = rho_harness_core::config::Config::default();
    let keyed = ProviderId::ALL
        .into_iter()
        .filter(|p| p.api_key_env().is_some_and(|env| std::env::var(env).is_err()));
    let logins = [ProviderId::ClaudeCode, ProviderId::ChatGpt, ProviderId::Antigravity];
    for provider in keyed.chain(logins) {
        let error = super::discover_models(provider.as_str(), &config, auth.clone())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Not logged in"), "{provider}: {error}");
    }
}

#[tokio::test]
async fn custom_provider_lists_models_from_its_base_url() {
    use rho_harness_core::config::{Config, ProviderConfig};
    let base = crate::test_support::serve_http(vec![(
        "200 OK",
        serde_json::json!({ "data": [{ "id": "acme-1", "context_length": 65_536 }] }).to_string(),
    )])
    .await;
    let (_dir, mut auth) = crate::test_support::temp_auth_store();
    auth.set_key("acme", "acme-key").unwrap();
    let mut config = Config::default();
    config.providers.insert(
        "acme".to_string(),
        ProviderConfig {
            base_url: format!("{base}/v1/"),
            ..Default::default()
        },
    );
    let models = super::discover_models("acme", &config, auth).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(
        (models[0].id.as_str(), models[0].context_tokens),
        ("acme-1", Some(65_536))
    );
}
