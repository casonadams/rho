use super::*;
use crate::test_support::temp_auth_store;
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
        default_model: None,
    }
}

fn shared_auth(auth_store: &AuthStore) -> std::sync::Arc<tokio::sync::Mutex<AuthStore>> {
    std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone()))
}

fn create_model_for_config(config: &Config, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
    ProviderFactory::create_model(config, model, auth_store, shared_auth(auth_store))
}

#[test]
fn custom_provider_builds_client_from_each_source() {
    let cases = [
        (
            config_with_provider("acme", spec("https://api.acme.dev/v1", None), false),
            "acme-large",
            Some(("ACME_API_KEY", "generic-env-secret")),
            None,
            "acme",
        ),
        (
            config_with_provider(
                "acme",
                spec("https://api.acme.dev/v1", Some("RHO_TEST_CUSTOM_PROVIDER_KEY")),
                false,
            ),
            "acme",
            Some(("RHO_TEST_CUSTOM_PROVIDER_KEY", "env-secret")),
            None,
            "acme",
        ),
        (
            config_with_provider("acme", spec("https://api.acme.dev/v1", None), false),
            "acme",
            None,
            Some(("acme", "stored-secret")),
            "acme",
        ),
        (
            config_with_provider(
                "custom_llm",
                spec("http://127.0.0.1:8080/v1", Some("LOCAL_API_KEY")),
                true,
            ),
            "custom_llm",
            Some(("LOCAL_API_KEY", "local-key")),
            None,
            "custom_llm",
        ),
        // A [providers.groq] entry would be rejected by config validation, so an
        // empty map proves the enum arm handled it.
        (
            Config {
                provider: "groq".to_string(),
                ..Default::default()
            },
            "llama-3.3-70b",
            None,
            Some(("groq", "gsk-test")),
            "groq",
        ),
    ];
    for (config, model, env, stored_key, expected) in cases {
        if let Some((var, value)) = env {
            unsafe {
                std::env::set_var(var, value);
            }
        }
        let (_dir, mut auth_store) = temp_auth_store();
        if let Some((provider, key)) = stored_key {
            auth_store.set_key(provider, key).unwrap();
        }

        let handle = create_model_for_config(&config, model, &auth_store).unwrap();
        assert_eq!(handle.provider_name(), expected);

        if let Some((var, _)) = env {
            unsafe {
                std::env::remove_var(var);
            }
        }
    }
}

#[test]
fn custom_provider_errors_name_the_failing_config() {
    let (_dir, auth_store) = temp_auth_store();
    let private = config_with_provider("custom_llm", spec("http://127.0.0.1:8080/v1", None), false);
    let unknown = Config {
        provider: "mystery".to_string(),
        providers: BTreeMap::new(),
        ..Default::default()
    };
    let cases = [
        (&private, "custom_llm", "blocked"),
        (&unknown, "mystery", "[providers.mystery]"),
    ];
    for (config, model, needle) in cases {
        let error = create_model_for_config(config, model, &auth_store).unwrap_err();
        assert!(error.to_string().contains(needle), "{error}");
    }
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
fn model_store_resolves_context_tokens_across_catalog_keys() {
    let dir = std::env::temp_dir().join(format!("rho_store_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = ModelStore::load(dir.join("models-store.json"));
    store
        .set_models(
            "ollama-cloud",
            vec![crate::provider::discovery::DiscoveredModel {
                id: "glm-5.3-flash".into(),
                name: "GLM 5.3 Flash".into(),
                provider: "ollama-cloud".into(),
                description: "1M ctx".into(),
                context_tokens: Some(1_048_576),
            }],
        )
        .unwrap();

    let store = ModelStore::load(dir.join("models-store.json"));
    assert_eq!(
        store.context_tokens(&["ollama-cloud"], "glm-5.3-flash"),
        Some(1_048_576)
    );
    assert_eq!(store.context_tokens(&["ollama-cloud"], "unknown"), None);

    std::fs::remove_dir_all(dir).unwrap();
}

fn make_discovered_models(ids: &[&str]) -> Vec<crate::provider::discovery::DiscoveredModel> {
    ids.iter()
        .map(|id| crate::provider::discovery::DiscoveredModel {
            context_tokens: None,
            id: id.to_string(),
            name: id.to_string(),
            provider: "antigravity".into(),
            description: String::new(),
        })
        .collect()
}

#[test]
fn antigravity_collapse_sorts_newest_first() {
    let live = [
        "gemini-2.5-flash",
        "gemini-3.8-flash-high",
        "gemini-3.7-flash-low",
        "gemini-3.8-flash-medium",
        "claude-sonnet-4-6",
        "gemini-2.5-pro",
        "gemini-3.1-pro-low",
    ];
    let models = crate::provider::discovery::sort_models_newest_first(make_discovered_models(&live));
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4-6",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-medium",
            "gemini-3.7-flash-low",
            "gemini-3.1-pro-low",
            "gemini-2.5-flash",
            "gemini-2.5-pro",
        ]
    );
}

#[test]
fn claude_provider_builds_model_handle() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut auth_store = AuthStore::load(dir.join("auth.json")).unwrap();
    auth_store
        .set_credential(
            "claude",
            rho_harness_core::auth::StoredCredential::OAuth {
                access_token: "test-access-token".into(),
                refresh_token: Some("test-refresh-token".into()),
                expires_at_ms: Some((chrono::Utc::now().timestamp() + 3600) * 1000),
                account_id: None,
                account_email: None,
            },
        )
        .unwrap();

    let request = ModelRequest {
        provider: ProviderId::ClaudeCode,
        model: "claude-sonnet-4-6",
        thinking_level: Some("high"),
        shared_auth: shared_auth(&auth_store),
    };
    let handle = ProviderFactory::create_model_for(request, &auth_store).unwrap();
    assert_eq!(handle.provider_name(), "claude-code");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn claude_provider_fails_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let auth_store = AuthStore::load(dir.path().join("auth.json")).unwrap();
    let request = ModelRequest {
        provider: ProviderId::ClaudeCode,
        model: "claude-sonnet-4-6",
        thinking_level: None,
        shared_auth: shared_auth(&auth_store),
    };
    let Err(err) = ProviderFactory::create_model_for(request, &auth_store) else {
        panic!("missing credentials must fail");
    };
    let err = err.to_string();
    assert!(err.contains("Missing API key for provider 'claude'") || err.contains("Run 'rho login claude'"));
}

#[test]
fn resolve_guard_model_tests() {
    let mut config = Config::default();
    assert_eq!(resolve_guard_model(&config), None);

    config.models.insert("guard".to_string(), "none".to_string());
    assert_eq!(resolve_guard_model(&config), None);

    config
        .models
        .insert("guard".to_string(), "local/qwen2.5-coder:7b".to_string());
    assert_eq!(
        resolve_guard_model(&config),
        Some((ProviderId::Local, "qwen2.5-coder:7b".to_string()))
    );

    config
        .models
        .insert("guard".to_string(), "ollama/qwen2.5-coder:7b".to_string());
    assert_eq!(
        resolve_guard_model(&config),
        Some((ProviderId::Local, "qwen2.5-coder:7b".to_string()))
    );

    config
        .models
        .insert("guard".to_string(), "anthropic/claude-3-5-haiku".to_string());
    assert_eq!(
        resolve_guard_model(&config),
        Some((ProviderId::Anthropic, "claude-3-5-haiku".to_string()))
    );

    config
        .models
        .insert("guard".to_string(), "qwen2.5-coder:7b".to_string());
    assert_eq!(
        resolve_guard_model(&config),
        Some((ProviderId::Local, "qwen2.5-coder:7b".to_string()))
    );
}

#[test]
fn test_guard_model_thinking_is_none() {
    let mut config = Config {
        thinking_level: Some("high".to_string()),
        ..Default::default()
    };
    config
        .models
        .insert("guard".to_string(), "local/qwen2.5-coder:7b".to_string());

    let (provider, model) = resolve_guard_model(&config).expect("guard model should resolve");
    let req = ProviderFactory::guard_model_request(provider, &model, shared_auth(&AuthStore::default()));
    assert_eq!(req.thinking_level, None);
    assert_eq!(req.provider, ProviderId::Local);
    assert_eq!(req.model, "qwen2.5-coder:7b");

    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();

    let handle = ProviderFactory::create_guard_model(&config, &auth_store, shared_auth(&auth_store)).unwrap();
    assert!(handle.is_some());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn guard_model_request_shares_the_engine_auth_store() {
    let shared = shared_auth(&AuthStore::default());
    let req = ProviderFactory::guard_model_request(ProviderId::Local, "qwen2.5-coder:7b", shared.clone());
    assert!(std::sync::Arc::ptr_eq(&req.shared_auth, &shared));
}

#[test]
fn test_factory_creates_model_from_spec() {
    let dir = std::env::temp_dir().join(format!("rho_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_store = AuthStore::load(dir.join("auth.json")).unwrap();

    // 1. Model spec overrides config.provider
    let config = Config {
        provider: "anthropic".to_string(),
        ..Default::default()
    };
    let handle =
        ProviderFactory::create_model(&config, "local/qwen2.5-coder:7b", &auth_store, shared_auth(&auth_store))
            .unwrap();
    assert_eq!(handle.provider_name(), "ollama-local");

    // 2. Bare model fallback to config.provider
    let config = Config {
        provider: "local".to_string(),
        ..Default::default()
    };
    let handle =
        ProviderFactory::create_model(&config, "qwen2.5-coder:7b", &auth_store, shared_auth(&auth_store)).unwrap();
    assert_eq!(handle.provider_name(), "ollama-local");

    // 3. Bare model without config.provider infers or falls back to local
    let config = Config {
        provider: String::new(),
        ..Default::default()
    };
    let handle =
        ProviderFactory::create_model(&config, "qwen2.5-coder:7b", &auth_store, shared_auth(&auth_store)).unwrap();
    assert_eq!(handle.provider_name(), "ollama-local");

    // 4. create_engine_model with canonical model spec
    let config = Config {
        provider: "local".to_string(),
        model: "local/qwen2.5-coder:7b".to_string(),
        ..Default::default()
    };
    let handle = crate::engine::builder::create_engine_model(&config, &auth_store, shared_auth(&auth_store)).unwrap();
    assert_eq!(handle.provider_name(), "ollama-local");

    // 5. Direct create_model_for with canonical model spec strips provider prefix
    let request = ModelRequest {
        provider: ProviderId::Local,
        model: "local/qwen2.5-coder:7b",
        thinking_level: None,
        shared_auth: shared_auth(&auth_store),
    };
    let handle = ProviderFactory::create_model_for(request, &auth_store).unwrap();
    assert_eq!(handle.provider_name(), "ollama-local");

    // 6. Test strip_provider_prefix directly across multiple providers
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::ChatGpt, "chatgpt/gpt-6-sol"),
        "gpt-6-sol"
    );
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::Antigravity, "antigravity/gemini-3.8-flash"),
        "gemini-3.8-flash"
    );
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::ClaudeCode, "claude/claude-sonnet-4-6"),
        "claude-sonnet-4-6"
    );
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::OpenRouter, "openrouter/anthropic/claude-3.7-sonnet"),
        "anthropic/claude-3.7-sonnet"
    );
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::OpenRouter, "anthropic/claude-3.7-sonnet"),
        "anthropic/claude-3.7-sonnet"
    );
    assert_eq!(
        ProviderFactory::strip_provider_prefix(ProviderId::ChatGpt, "gpt-6-sol"),
        "gpt-6-sol"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn openai_complete_sends_exactly_one_request() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let _ = socket.read(&mut buf).await;
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    let client = OpenAiClient::new(format!("http://{addr}"), "key", "gpt-test", "test");
    let completion = client
        .complete(ModelCompletionRequest {
            prompt: "hi".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    server.abort();

    assert_eq!(completion.content, "hi");
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[test]
fn openai_client_debug_redacts_api_key_and_extra_headers() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-api-token",
        reqwest::header::HeaderValue::from_static("header-secret"),
    );
    let client =
        OpenAiClient::new("https://api.example.com/v1", "sk-super-secret", "gpt-test", "example").with_headers(headers);
    let debug = format!("{client:?}");
    assert!(!debug.contains("sk-super-secret"), "{debug}");
    assert!(!debug.contains("header-secret"), "{debug}");
    assert!(debug.contains("gpt-test"));
}
