use super::*;
use rho_harness_core::config::ProviderConfig;
use std::collections::BTreeMap;

fn config_with_provider(name: &str, spec: ProviderConfig, allow_private: bool) -> Config {
    let mut config = Config {
        provider: name.to_string(),
        allow_private_network: allow_private,
        ..Default::default()
    };
    config.providers.insert(name.to_string(), spec);
    config
}

fn spec(base_url: &str, key_env: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        base_url: base_url.to_string(),
        key_env: key_env.map(str::to_string),
    }
}

#[test]
fn custom_provider_builds_generic_client_from_config() {
    unsafe {
        std::env::set_var("ACME_API_KEY", "generic-env-secret");
    }
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    let config = config_with_provider("acme", spec("https://api.acme.dev/v1", None), false);

    let handle = ProviderFactory::create_model(&config, "acme-large", &auth_store).unwrap();
    assert_eq!(handle.label(), Some("acme"));

    std::fs::remove_dir_all(dir).unwrap();
    unsafe {
        std::env::remove_var("ACME_API_KEY");
    }
}

#[test]
fn custom_provider_resolves_key_from_key_env() {
    unsafe {
        std::env::set_var("RHO_TEST_CUSTOM_PROVIDER_KEY", "env-secret");
    }
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    let config = config_with_provider(
        "acme",
        spec("https://api.acme.dev/v1", Some("RHO_TEST_CUSTOM_PROVIDER_KEY")),
        false,
    );

    let handle = ProviderFactory::create_model(&config, "acme", &auth_store).unwrap();
    assert_eq!(handle.label(), Some("acme"));

    std::fs::remove_dir_all(dir).unwrap();
    unsafe {
        std::env::remove_var("RHO_TEST_CUSTOM_PROVIDER_KEY");
    }
}

#[test]
fn custom_provider_resolves_key_from_auth_store() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    auth_store.set_key("acme", "stored-secret").unwrap();
    let config = config_with_provider("acme", spec("https://api.acme.dev/v1", None), false);

    ProviderFactory::create_model(&config, "acme", &auth_store).unwrap();

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn custom_provider_rejects_private_base_url_by_default() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    let config = config_with_provider("custom_llm", spec("http://127.0.0.1:8080/v1", None), false);

    let error = ProviderFactory::create_model(&config, "custom_llm", &auth_store).unwrap_err();
    assert!(error.to_string().contains("blocked"), "{error}");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn custom_provider_allows_private_base_url_when_enabled() {
    unsafe {
        std::env::set_var("LOCAL_API_KEY", "local-key");
    }
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    let config = config_with_provider(
        "custom_llm",
        spec("http://127.0.0.1:8080/v1", Some("LOCAL_API_KEY")),
        true,
    );

    ProviderFactory::create_model(&config, "custom_llm", &auth_store).unwrap();

    std::fs::remove_dir_all(dir).unwrap();
    unsafe {
        std::env::remove_var("LOCAL_API_KEY");
    }
}

#[test]
fn unknown_provider_without_config_errors_with_guidance() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    let config = Config::default();
    let config = Config {
        provider: "mystery".to_string(),
        providers: BTreeMap::new(),
        ..config
    };

    let error = ProviderFactory::create_model(&config, "mystery", &auth_store).unwrap_err();
    assert!(error.to_string().contains("[providers.mystery]"));

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn well_known_provider_still_routes_to_dedicated_arms() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    auth_store.set_key("groq", "gsk-test").unwrap();
    let config = Config {
        provider: "groq".to_string(),
        ..Default::default()
    };
    // A [providers.groq] entry would be rejected by config validation, so an
    // empty map proves the enum arm handled it.
    ProviderFactory::create_model(&config, "llama-3.3-70b", &auth_store).unwrap();

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ollama_context_length_is_read_from_model_info() {
    use crate::provider::discovery::ollama_context_from_info;

    let info: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(r#"{"qwen3_5.context_length": 262144, "qwen3_5.embedding_length": 5120}"#).unwrap();
    assert_eq!(ollama_context_from_info(&info), Some(262_144));

    let empty: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    assert_eq!(ollama_context_from_info(&empty), None);
}

#[test]
fn antigravity_collapse_sorts_newest_first() {
    let live_ids: Vec<String> = vec![
        "gemini-2.5-flash".into(),
        "gemini-3.8-flash-high".into(),
        "gemini-3.7-flash-low".into(),
        "gemini-3.8-flash-medium".into(),
        "claude-sonnet-4-6".into(),
        "gemini-2.5-pro".into(),
        "gemini-3.1-pro-low".into(),
    ];
    let models = crate::provider::discovery::sort_models_newest_first(
        live_ids
            .iter()
            .map(|id| crate::provider::discovery::DiscoveredModel {
                context_tokens: None,
                id: id.clone(),
                name: id.clone(),
                provider: "antigravity".into(),
                description: String::new(),
            })
            .collect(),
    );
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4-6",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-medium",
            "gemini-3.7-flash-low",
            "gemini-3.1-pro-low",
            "gemini-2.5-flash", // (2,5) tie keeps input order
            "gemini-2.5-pro",
        ]
    );
}
