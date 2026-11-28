use super::*;
use crate::auth::AuthStore;
use crate::mailbox::AgentId;
use crate::mcp::load_mcp_tools;
use crate::test_support::temp_dir;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use rho_test_support::EnvVarGuard;
use std::collections::BTreeMap;
use std::path::Path;

fn test_config(label: &str) -> (Config, crate::test_support::TestDir) {
    let dir = temp_dir(label);
    let config = Config {
        sessions_dir: dir.join("sessions"),
        auth_file: dir.join("auth.json"),
        ..Default::default()
    };
    (config, dir)
}

// Engine construction goes through ProviderFactory; a dummy key is fine because
// client construction never contacts the network.
fn with_dummy_provider_key() -> EnvVarGuard {
    EnvVarGuard::set("ANTHROPIC_API_KEY", "test-key-not-real")
}

fn mock_mcp_server(workspace: &Path) -> (String, String) {
    let script = workspace.join(format!("mock_mcp_server_{}.sh", uuid::Uuid::new_v4().simple()));
    std::fs::write(
        &script,
        r#"#!/bin/sh
while IFS= read -r line; do
    if echo "$line" | grep -q '"method":"initialize"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"mock\",\"version\":\"1.0\"}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"ping\",\"description\":\"Mock ping\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    fi
done
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let command = script.to_str().unwrap().to_string();
    let pattern = script.file_name().unwrap().to_string_lossy().to_string();
    (command, pattern)
}

fn with_mock_mcp(mut config: Config, command: String) -> Config {
    let mut servers = BTreeMap::new();
    let mut server = McpServerConfig::stdio(command, Vec::new());
    server.lifecycle = Some(rho_harness_core::config::McpLifecycleMode::Eager);
    servers.insert("mock".to_string(), server);
    config.mcp = McpConfig {
        enabled: true,
        defer_threshold: 10,
        idle_timeout_seconds: 600,
        startup_timeout_ms: 0,
        servers,
        project: None,
    };
    config
}

fn count_server_processes(pattern: &str) -> usize {
    let output = std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

async fn wait_for_server_count(pattern: &str, expected: usize) {
    for _ in 0..40 {
        if count_server_processes(pattern) == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!(
        "expected {expected} mock MCP server process(es) matching '{pattern}', found {}",
        count_server_processes(pattern)
    );
}

async fn seed_test_engine_history(engine: &AgentEngine) -> (Vec<rho_harness_core::model::ChatMessage>, Vec<u8>) {
    let sid = &engine.session_manager.session_id;
    let msgs = vec![
        rho_harness_core::model::ChatMessage::user("remember this line"),
        rho_harness_core::model::ChatMessage::assistant("recorded"),
    ];
    engine.session_manager.append_messages(sid, msgs).await.unwrap();
    let history = engine.session_manager.load_messages().await.unwrap();
    let jsonl = engine.config.sessions_dir.join(format!("{sid}.jsonl"));
    let bytes = std::fs::read(&jsonl).unwrap();
    (history, bytes)
}

fn assert_rebuilt_meta(rebuilt: &AgentEngine, sid: &str, tool_count: usize) {
    assert_eq!(
        (rebuilt.config.max_turns, rebuilt.session_manager.session_id.as_str()),
        (7, sid)
    );
    let names = rebuilt.tool_names();
    assert_eq!(names.len(), tool_count + 2, "{names:?}");
    assert!(names.iter().any(|n| n == "worktree"), "{names:?}");
    assert!(names.iter().any(|n| n == "lsp"), "{names:?}");
}

async fn assert_rebuilt_storage(
    rebuilt: &AgentEngine,
    sid: &str,
    history: &[rho_harness_core::model::ChatMessage],
    jsonl: &[u8],
) {
    assert_eq!(rebuilt.session_manager.load_messages().await.unwrap(), history);
    assert_eq!(
        std::fs::read(rebuilt.config.sessions_dir.join(format!("{sid}.jsonl"))).unwrap(),
        jsonl
    );
}

struct BuiltinEngine {
    engine: AgentEngine,
    config: Config,
    auth_store: AuthStore,
    tool_count: usize,
    dir: crate::test_support::TestDir,
}

async fn builtin_engine(label: &str) -> BuiltinEngine {
    let (config, dir) = test_config(label);
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();
    let built = crate::tools::build_builtin_tools(&base_dir, &config, None).unwrap();
    let tool_count = built.tools.len();
    let tools_runtime = built.runtime.clone();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir)
        .builtin_tools(built)
        .build()
        .await
        .unwrap();
    assert_eq!(engine.tool_names().len(), tool_count);
    assert!(
        engine.subagent_runtime.shares_state_with(&tools_runtime),
        "the engine reaches the subagents its built-in tools start"
    );
    BuiltinEngine {
        engine,
        config,
        auth_store,
        tool_count,
        dir,
    }
}

#[tokio::test]
async fn rebuild_preserves_session_history_and_reattaches_tools() {
    let _env = with_dummy_provider_key();
    let BuiltinEngine {
        engine,
        config,
        auth_store,
        tool_count,
        dir: _dir,
    } = builtin_engine("continuity").await;
    let session_id = engine.session_manager.session_id.clone();
    let (history_before, jsonl_before) = seed_test_engine_history(&engine).await;

    let mut new_config = config.clone();
    new_config.max_turns = 7;
    let rebuilt = engine.rebuild(new_config, auth_store).await.unwrap();

    assert_rebuilt_meta(&rebuilt, &session_id, tool_count);
    assert_rebuilt_storage(&rebuilt, &session_id, &history_before, &jsonl_before).await;
    assert!(
        rebuilt.subagent_runtime.shares_state_with(&engine.subagent_runtime),
        "running subagents stay counted and reachable after a rebuild"
    );
}

async fn setup_mcp_engine(config: Config, workspace: &std::path::Path, auth_store: AuthStore) -> AgentEngine {
    let tools = load_mcp_tools(&config, workspace).await;
    assert!(tools.iter().any(|t| t.name() == "mock_ping"));
    builder::AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.to_path_buf())
        .tools(tools)
        .build()
        .await
        .unwrap()
}

/// Rebuild must re-resolve MCP tools (REQ-004) and dropping the old engine must
/// reap its MCP child (no process leak across reloads).
#[cfg(unix)]
#[tokio::test]
async fn rebuild_respawns_mcp_tools_and_reaps_previous_children() {
    let _env = with_dummy_provider_key();
    let (config, dir) = test_config("mcp_rebuild");
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (script_command, script_pattern) = mock_mcp_server(&workspace);
    let config = with_mock_mcp(config, script_command);

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = setup_mcp_engine(config.clone(), &workspace, auth_store.clone()).await;
    wait_for_server_count(&script_pattern, 1).await;

    let rebuilt = engine.rebuild(config.clone(), auth_store.clone()).await.unwrap();
    assert!(rebuilt.tool_names().iter().any(|name| name == "mock_ping"));
    drop(engine);
    wait_for_server_count(&script_pattern, 1).await;
}

/// Repeated reloads stay at one live MCP child per configured server.
#[cfg(unix)]
#[tokio::test]
async fn repeated_rebuilds_do_not_leak_mcp_children() {
    let _env = with_dummy_provider_key();
    let (config, dir) = test_config("mcp_no_leak");
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (script_command, script_pattern) = mock_mcp_server(&workspace);
    let config = with_mock_mcp(config, script_command);

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(workspace.clone())
        .lsp(None)
        .build()
        .await
        .unwrap();

    let mut current = engine;
    for _ in 0..3 {
        current = current.rebuild(config.clone(), auth_store.clone()).await.unwrap();
        wait_for_server_count(&script_pattern, 1).await;
    }
}

#[tokio::test]
async fn builder_attaches_dynamic_tools() {
    let _env = with_dummy_provider_key();
    let (config, _dir) = test_config("custom_tools");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();

    let custom_tool = Arc::new(crate::tools::GenericTool::new(
        "generate_image",
        "Generate image tool",
        serde_json::json!({
            "type": "object",
            "properties": { "prompt": { "type": "string" } },
            "required": ["prompt"]
        }),
        |_args| async { Ok(crate::tools::ToolResult::success("image.png")) },
    ));

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir)
        .lsp(None)
        .add_tool(custom_tool)
        .build()
        .await
        .unwrap();

    assert!(engine.tool_names().contains(&"generate_image".to_string()));
    assert!(engine.tool_names().contains(&"read".to_string()));
    assert!(engine.tool_names().contains(&"bash".to_string()));
}

#[tokio::test]
async fn refresh_quota_non_antigravity_is_noop() {
    let _env = with_dummy_provider_key();
    let (config, _dir) = test_config("quota_noop");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(std::env::temp_dir())
        .lsp(None)
        .build()
        .await
        .unwrap();

    assert_eq!(engine.quota_display(), None);
    engine.refresh_quota().await;
    assert_eq!(engine.quota_display(), None);
}

#[tokio::test]
async fn build_engine_offline_with_expired_oauth_token_succeeds_without_blocking() {
    let (config, dir) = test_config("offline_expired_oauth");
    let config = Config {
        provider: "antigravity".to_string(),
        model: "gemini-2.5-flash".to_string(),
        ..config
    };

    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store
        .set_credential(
            "antigravity",
            rho_harness_core::auth::StoredCredential::OAuth {
                access_token: "expired-token".into(),
                refresh_token: Some("dummy-refresh-token".into()),
                expires_at_ms: Some(1000),
                account_id: Some("test-project".into()),
                account_email: None,
            },
        )
        .unwrap();

    let start = std::time::Instant::now();
    let engine = builder::AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir.to_path_buf())
        .lsp(None)
        .quota_endpoints(crate::engine::quota_refresh::QuotaEndpoints::all_at(
            "http://127.0.0.1:9",
        ))
        .build()
        .await
        .expect("Engine build must succeed offline without blocking on OAuth refresh");

    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "Engine build must not block or hang on network when token is expired"
    );

    engine.spawn_refresh_quota();
    assert!(!engine.tool_names().is_empty());
}

/// The offline mock engine proves the no-key path never touches the network.
#[tokio::test]
async fn refresh_quota_ollama_cloud_without_key_stays_empty() {
    let (config, dir) = test_config("quota_ollama_nokey");
    let config = Config {
        provider: "ollama-cloud".to_string(),
        ..config
    };
    let engine = crate::engine::eval::mock::mock_engine(
        Arc::new(crate::provider::adapter::MockAdapter::default()),
        crate::engine::eval::mock::MockEngineConfig::new(&dir, config),
    );

    engine.refresh_quota().await;
    assert_eq!(engine.quota_display(), None);
}

#[tokio::test]
async fn refresh_quota_gemini_populates_billing_window_cost() {
    let (config, dir) = test_config("quota_gemini");
    let config = Config {
        provider: "gemini".to_string(),
        model: "gemini-2.5-flash".to_string(),
        ..config
    };
    let engine = crate::engine::eval::mock::mock_engine(
        Arc::new(crate::provider::adapter::MockAdapter::default()),
        crate::engine::eval::mock::MockEngineConfig::new(&dir, config),
    );

    assert_eq!(engine.quota_display(), None);
    engine.refresh_quota().await;
    assert_eq!(engine.quota_display(), Some("$0.000 • $0.000".to_string()));
}

fn mock_quota_engine(dir: &std::path::Path, provider: &str, model: &str) -> crate::engine::AgentEngine {
    crate::engine::eval::mock::mock_engine(
        Arc::new(crate::provider::adapter::MockAdapter::default()),
        crate::engine::eval::mock::MockEngineConfig::new(
            dir,
            Config {
                provider: provider.to_string(),
                model: model.to_string(),
                ..Config::default()
            },
        ),
    )
}

#[tokio::test]
async fn quota_display_omitted_for_unsupported_provider() {
    let (_, dir) = test_config("quota_unsupported");
    let mut engine = mock_quota_engine(&dir, "antigravity", "gemini-2.5-pro");
    let ag_key = crate::engine::tracking::QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
    engine.quota.record_success(&ag_key, "85% (3h22m)".to_string());
    assert_eq!(engine.quota_display(), Some("85% (3h22m)".to_string()));

    engine.config.provider = "local".to_string();
    assert_eq!(engine.quota_display(), None);
}

#[tokio::test]
async fn quota_display_isolated_across_providers_and_models() {
    let (_, dir) = test_config("quota_isolation");
    let mut engine = mock_quota_engine(&dir, "antigravity", "gemini-2.5-pro");
    let ag_key = crate::engine::tracking::QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
    engine.quota.record_success(&ag_key, "85% (3h22m)".to_string());

    engine.config.provider = "ollama-cloud".to_string();
    assert_eq!(engine.quota_display(), None);
    let ollama_key = crate::engine::tracking::QuotaKey::new("ollama-cloud", None::<String>);
    engine.quota.record_success(&ollama_key, "20% used".to_string());
    assert_eq!(engine.quota_display(), Some("20% used".to_string()));

    engine.config.provider = "google-antigravity".to_string();
    assert_eq!(engine.quota_display(), Some("85% (3h22m)".to_string()));
    engine.config.model = "claude-sonnet-4-6".to_string();
    assert_eq!(engine.quota_display(), None);

    engine.config.provider = "chatgpt".to_string();
    engine.config.model = "gpt-5.4".to_string();
    assert_eq!(engine.quota_display(), None);
    let chatgpt_key = crate::engine::tracking::QuotaKey::new("chatgpt", None::<String>);
    engine
        .quota
        .record_success(&chatgpt_key, "95% 4h23m 89% 3d21h".to_string());
    assert_eq!(engine.quota_display(), Some("95% 4h23m 89% 3d21h".to_string()));

    engine.config.provider = "claude".to_string();
    engine.config.model = "claude-sonnet-4-6".to_string();
    assert_eq!(engine.quota_display(), None);
    let claude_key = crate::engine::tracking::QuotaKey::new("claude", Some("claude-sonnet-4-6"));
    engine
        .quota
        .record_success(&claude_key, "100% 3h15m 68% 4d11h".to_string());
    assert_eq!(engine.quota_display(), Some("100% 3h15m 68% 4d11h".to_string()));
    engine.config.provider = "claude-code".to_string();
    assert_eq!(engine.quota_display(), Some("100% 3h15m 68% 4d11h".to_string()));
    engine.config.provider = "claude-oauth".to_string();
    assert_eq!(engine.quota_display(), Some("100% 3h15m 68% 4d11h".to_string()));
    engine.config.provider = "claude:me@work.com".to_string();
    assert_eq!(engine.quota_display(), None);
    let work_key = crate::engine::tracking::QuotaKey::new("claude:me@work.com", Some("claude-sonnet-4-6"));
    engine.quota.record_success(&work_key, "40% 1h 10% 6d".to_string());
    assert_eq!(engine.quota_display(), Some("40% 1h 10% 6d".to_string()));
    engine.config.provider = "claude".to_string();
    assert_eq!(engine.quota_display(), Some("100% 3h15m 68% 4d11h".to_string()));

    engine.config.provider = "gemini".to_string();
    engine.config.model = "gemini-2.5-flash".to_string();
    assert_eq!(engine.quota_display(), None);
    let gemini_key = crate::engine::tracking::QuotaKey::new("gemini", Some("gemini-2.5-flash"));
    engine.quota.record_success(&gemini_key, "$0.025 • $0.150".to_string());
    assert_eq!(engine.quota_display(), Some("$0.025 • $0.150".to_string()));
    engine.config.provider = "google".to_string();
    assert_eq!(engine.quota_display(), Some("$0.025 • $0.150".to_string()));
}

#[tokio::test]
async fn context_limit_uses_the_providers_listed_window() {
    let (config, dir) = test_config("ctx_ollama_cloud");
    let config = Config {
        provider: "ollama-cloud".to_string(),
        model: "glm-5.3-flash".to_string(),
        ..config
    };
    let catalog = crate::provider::ModelCatalog::default();
    let model = crate::provider::discovery::DiscoveredModel {
        context_tokens: Some(1_048_576),
        ..crate::provider::discovery::DiscoveredModel::new("glm-5.3-flash", "GLM", "")
    };
    catalog.set("ollama-cloud", crate::provider::ProviderModels::Ready(vec![model]));
    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store.set_key("ollama-cloud", "test-key-not-real").unwrap();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store)
        .base_dir(dir.to_path_buf())
        .lsp(None)
        .model_catalog(catalog)
        .build()
        .await
        .unwrap();
    assert_eq!(engine.context_limit(), 1_048_576);
    assert_eq!(
        engine.context_limit_source(),
        crate::engine::tracking::ContextLimitSource::Provider
    );
}

#[tokio::test]
async fn subagent_bare_model_stays_on_the_session_provider_that_lists_it() {
    let (config, _dir) = test_config("subagent_ollama_cloud");
    let config = Config {
        provider: "ollama-cloud".to_string(),
        model: "glm-5.3-flash".to_string(),
        ..config
    };
    let catalog = crate::provider::ModelCatalog::default();
    let listed = crate::provider::discovery::DiscoveredModel::new("deepseek-v4.1-flash", "DeepSeek", "");
    catalog.set("ollama-cloud", crate::provider::ProviderModels::Ready(vec![listed]));
    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store.set_key("ollama-cloud", "test-key-not-real").unwrap();
    let router = crate::provider::ModelRouter::new(Arc::new(tokio::sync::Mutex::new(auth_store)), catalog);
    let resolve = builder::subagent_model_resolver(&router, &config);
    let built = tokio::task::spawn_blocking(move || resolve("deepseek-v4.1-flash").map(|_| ()))
        .await
        .unwrap();
    assert_eq!(built, Ok(()));
}

#[tokio::test]
async fn explicit_model_prevents_silent_fallback_to_configured_provider() {
    let (config, dir) = test_config("no_fallback_explicit");
    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store.set_key("deepseek", "dummy-deepseek-key").unwrap();

    let config = Config {
        provider: "nonexistent-provider".to_string(),
        model: "nonexistent-model".to_string(),
        default_model: Some("nonexistent-provider/nonexistent-model".to_string()),
        ..config
    };

    let result = builder::AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir.to_path_buf())
        .lsp(None)
        .build()
        .await;

    assert!(
        result.is_err(),
        "Must not fall back to deepseek when explicit model is set"
    );
}

#[tokio::test]
async fn switch_model_updates_model_and_preserves_tools() {
    let _env = with_dummy_provider_key();
    let BuiltinEngine {
        mut engine,
        config,
        tool_count,
        dir: _dir,
        ..
    } = builtin_engine("switch_model").await;

    assert_eq!(engine.config.model, config.model);
    assert_eq!(engine.tool_names().len(), tool_count);

    engine
        .switch_model("claude-3-5-haiku-20241022", "anthropic")
        .await
        .unwrap();

    assert_eq!(engine.config.model, "claude-3-5-haiku-20241022");
    assert_eq!(engine.config.provider, "anthropic");
    assert_eq!(engine.tool_names().len(), tool_count);
    assert_eq!(engine.context_limit(), 200_000);
}

#[tokio::test(start_paused = true)]
async fn rate_limited_model_request_retries_and_succeeds() {
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
    use rho_harness_core::error::AppError;

    let dir = crate::test_support::TestDir::new();

    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Err(AppError::RateLimited {
        message: "rate limited".to_string(),
        retry_after: None,
    })]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text(
        "recovered successfully after rate limit".to_string(),
    ))]);

    let app_config = Config {
        keep_recent_tokens: 5,
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &dir,
            app_config,
            session_manager: None,
            built_in_tools: None,
        },
    );

    let presenter = Arc::new(crate::test_support::CapturingPresenter::default());
    let output = engine
        .run_turn(
            crate::engine::runner::TurnRequest::new("Hello after rate limit"),
            presenter.clone(),
        )
        .await
        .expect("turn should succeed after retry");

    assert_eq!(output.final_text, "recovered successfully after rate limit");
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(
        notices
            .iter()
            .any(|n| n.contains("rate limited") && n.contains("retrying in 2s (1/2)")),
        "expected rate limit notice, got: {notices:?}"
    );
}

#[tokio::test]
async fn a_handed_off_workflow_report_precedes_only_the_next_prompt() {
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
    let dir = crate::test_support::TestDir::new();
    let model = Arc::new(MockAdapter::default());
    for reply in ["reviewed", "next"] {
        model.add_turn(vec![Ok(ModelStreamEvent::Text(reply.to_string()))]);
    }
    let engine = mock_engine(
        model.clone(),
        MockEngineConfig::new(
            &dir,
            Config {
                auth_file: dir.join("auth.json"),
                ..Config::default()
            },
        ),
    );
    engine.hand_off_workflow_report("refine", "Workflow `refine` stopped.\n\nAgent tasks ...");
    let presenter = Arc::new(crate::engine::eval::presenter::NoopPresenter);

    for prompt in ["check it", "thanks"] {
        engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await
            .unwrap();
    }

    let prompts: Vec<String> = model
        .requests()
        .into_iter()
        .map(|r| r.messages.last().and_then(|m| m.text()).unwrap_or_default().to_string())
        .collect();
    assert!(
        prompts[0].contains("[The user ran workflow `refine`; its report follows."),
        "{}",
        prompts[0]
    );
    assert!(
        prompts[0].ends_with("Workflow `refine` stopped.\n\nAgent tasks ...\n\ncheck it"),
        "{}",
        prompts[0]
    );
    assert!(prompts[1].ends_with("thanks"));
    assert!(!prompts[1].contains("[The user ran workflow"), "{}", prompts[1]);
}

#[tokio::test]
async fn each_turn_request_carries_the_session_id_as_its_conversation_id() {
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
    let dir = crate::test_support::TestDir::new();
    let model = Arc::new(MockAdapter::default());
    for reply in ["one", "two"] {
        model.add_turn(vec![Ok(ModelStreamEvent::Text(reply.to_string()))]);
    }
    let engine = mock_engine(
        model.clone(),
        MockEngineConfig::new(
            &dir,
            Config {
                auth_file: dir.join("auth.json"),
                ..Config::default()
            },
        ),
    );
    let presenter = Arc::new(crate::engine::eval::presenter::NoopPresenter);

    for prompt in ["first", "second"] {
        engine
            .run_turn(crate::engine::runner::TurnRequest::new(prompt), presenter.clone())
            .await
            .unwrap();
    }

    let ids: Vec<Option<String>> = model
        .requests()
        .into_iter()
        .map(|r| r.conversation_id)
        .take(2)
        .collect();
    assert_eq!(ids, vec![Some(engine.session_manager.session_id.clone()); 2]);
}

fn mock_engine_in(dir: &Path, session: Option<SessionManager>) -> AgentEngine {
    crate::engine::eval::mock::mock_engine(
        Arc::new(crate::provider::adapter::MockAdapter::default()),
        crate::engine::eval::mock::MockEngineConfig {
            session_manager: session,
            ..crate::engine::eval::mock::MockEngineConfig::new(dir, Config::default())
        },
    )
}

#[tokio::test]
async fn an_interrupt_stops_this_sessions_background_jobs_but_not_another_sessions() {
    let dir = crate::test_support::TestDir::new();
    let engine = mock_engine_in(&dir, None);
    let id = AgentId::from(engine.session_manager.session_id.as_str());
    let jobs = engine.inbox().jobs().clone();
    let own = jobs.register(&id, format!("{id}-job").into(), "sleep".into());
    let other = jobs.register(&"interrupt-other".into(), "interrupt-other-job".into(), "sleep".into());

    engine.record_cancellation("operator interrupt").await.unwrap();

    assert!(own.is_cancelled());
    assert!(!other.is_cancelled());
}

#[tokio::test]
async fn switching_sessions_stops_the_left_sessions_background_work_but_not_the_new_ones() {
    let dir = crate::test_support::TestDir::new();
    let mut engine = mock_engine_in(&dir.join("a"), None);
    let next = mock_engine_in(&dir.join("b"), None);
    let (left, arrived) = (engine.inbox(), next.inbox());
    assert_ne!(left.id(), arrived.id());
    let left_job = left.jobs().register(left.id(), "left-job".into(), "sleep".into());
    let arrived_job = arrived
        .jobs()
        .register(arrived.id(), "arrived-job".into(), "sleep".into());
    let left_mail = left.registry().clone();
    left.notices().send("stale notice").unwrap();
    arrived.notices().send("fresh notice").unwrap();
    let left_id = left.id().clone();
    drop((left, arrived));

    engine.switch_to(next);

    assert_ne!(engine.session_manager.session_id, left_id.as_str());
    assert!(left_job.is_cancelled());
    assert!(!arrived_job.is_cancelled());
    assert!(
        !left_mail.is_open(&left_id),
        "the left session's mailbox and its notices are gone"
    );
    assert_eq!(engine.inbox().take()[0].body, "fresh notice");
}

#[tokio::test]
async fn rebuilding_the_same_session_keeps_its_background_work() {
    let dir = crate::test_support::TestDir::new();
    let session = SessionManager::new(&dir.join("sessions"), None).unwrap();
    let id = AgentId::from(session.session_id.as_str());
    let mut engine = mock_engine_in(&dir, Some(session.clone()));
    let mut rebuilt = mock_engine_in(&dir, Some(session));
    rebuilt.subagent_runtime = engine.subagent_runtime.clone();
    let job = engine
        .inbox()
        .jobs()
        .register(&id, format!("{id}-job").into(), "sleep".into());
    engine.inbox().notices().send("kept notice").unwrap();

    engine.switch_to(rebuilt);

    assert!(!job.is_cancelled());
    assert_eq!(engine.inbox().take()[0].body, "kept notice");
}
