use super::*;
use crate::auth::AuthStore;
use crate::test_support::temp_auth_store;
use rho_harness_core::config::{Config, ProviderConfig};
use rho_harness_core::error::Result;
use rho_harness_core::provider::{ModelRoute, ProviderId};
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

fn router(auth_store: &AuthStore) -> ModelRouter {
    ModelRouter::new(
        std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone())),
        ModelCatalog::default(),
    )
}

fn create_model_for_config(config: &Config, model: &str, auth_store: &AuthStore) -> Result<ModelHandle> {
    router(auth_store).build_blocking(config, &ModelRoute::new(&config.provider, model))
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

fn make_discovered_models(ids: &[&str]) -> Vec<crate::provider::discovery::DiscoveredModel> {
    ids.iter()
        .map(|id| crate::provider::discovery::DiscoveredModel::new(id, id, ""))
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

    let config = Config {
        thinking_level: Some("high".to_string()),
        ..Default::default()
    };
    let handle = router(&auth_store)
        .build_blocking(&config, &ModelRoute::new("claude", "claude-sonnet-4-6"))
        .unwrap();
    assert_eq!(handle.provider_name(), "claude");

    let api_key = builders::build_api_key_model(ProviderId::Anthropic, "claude-sonnet-4-6", "k".into()).unwrap();
    assert_eq!(api_key.provider_name(), "anthropic");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn claude_provider_fails_without_credentials() {
    let (_dir, auth_store) = temp_auth_store();
    let Err(err) =
        router(&auth_store).build_blocking(&Config::default(), &ModelRoute::new("claude", "claude-sonnet-4-6"))
    else {
        panic!("missing credentials must fail");
    };
    let err = err.to_string();
    assert!(err.contains("Missing API key for provider 'claude'") || err.contains("Run 'rho login claude'"));
}

#[test]
fn guard_route_reads_the_guard_role() {
    let cases = [
        (None, None),
        (Some("none"), None),
        (Some("local/qwen2.5-coder:7b"), Some("local/qwen2.5-coder:7b")),
        (Some("ollama/qwen2.5-coder:7b"), Some("local/qwen2.5-coder:7b")),
        (Some("anthropic/claude-3-5-haiku"), Some("anthropic/claude-3-5-haiku")),
        (Some("qwen2.5-coder:7b"), Some("local/qwen2.5-coder:7b")),
        (Some("acme/m-1"), Some("acme/m-1")),
    ];
    for (spec, expected) in cases {
        let mut config = Config::default();
        if let Some(spec) = spec {
            config.models.insert("guard".to_string(), spec.to_string());
        }
        assert_eq!(
            guard_route(&config).map(|r| r.to_string()).as_deref(),
            expected,
            "{spec:?}"
        );
    }
}

#[tokio::test]
async fn guard_model_builds_only_when_configured_and_releases_the_auth_lock() {
    let (_dir, auth_store) = temp_auth_store();
    let router = router(&auth_store);
    let mut config = Config {
        thinking_level: Some("high".to_string()),
        ..Default::default()
    };
    assert!(router.build_guard(&config).await.unwrap().is_none());

    config
        .models
        .insert("guard".to_string(), "local/qwen2.5-coder:7b".to_string());
    let guard = router.build_guard(&config).await.unwrap().unwrap();
    assert_eq!(
        (guard.provider_name(), guard.model_name()),
        ("local", "qwen2.5-coder:7b")
    );
    assert!(router.auth().try_lock().is_ok());
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn slow_command_key_resolution_keeps_the_runtime_responsive() {
    let (_dir, mut store) = temp_auth_store();
    store
        .set_credential(
            "anthropic",
            rho_harness_core::auth::StoredCredential::api_key("!sleep 0.2; echo sk-ant-slow".to_string()),
        )
        .unwrap();
    let router = router(&store);
    let started = std::time::Instant::now();
    let resolving = tokio::spawn(async move {
        router
            .build(&Config::default(), &ModelRoute::new("anthropic", "claude-3.7-sonnet"))
            .await
    });
    tokio::task::yield_now().await;
    assert!(started.elapsed() < std::time::Duration::from_millis(150));
    let _model = resolving.await.unwrap().unwrap();
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

#[test]
fn gemini_and_chatgpt_builders_name_provider_and_model() {
    let gemini = builders::build_api_key_model(ProviderId::Gemini, "gemini-2.5-pro", "gem-key".to_string()).unwrap();
    assert_eq!(
        (gemini.provider_name(), gemini.model_name()),
        ("gemini", "gemini-2.5-pro")
    );

    let (_dir, mut auth_store) = temp_auth_store();
    auth_store.set_key("chatgpt", "tok").unwrap();
    let chatgpt = router(&auth_store)
        .build_blocking(&Config::default(), &ModelRoute::new("chatgpt", "gpt-5.4"))
        .unwrap();
    assert_eq!((chatgpt.provider_name(), chatgpt.model_name()), ("chatgpt", "gpt-5.4"));
}
