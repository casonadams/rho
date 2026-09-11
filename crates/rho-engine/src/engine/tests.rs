use super::*;
use crate::auth::AuthStore;
use crate::mcp::load_mcp_tools;
use rho_harness_core::config::{Config, McpConfig, McpServerConfig};
use rig::memory::ConversationMemory;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("engine_{label}_{}", uuid::Uuid::new_v4()))
}

fn test_config(label: &str) -> (Config, PathBuf) {
    let dir = temp_dir(label);
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config {
        sessions_dir: dir.join("sessions"),
        auth_file: dir.join("auth.json"),
        ..Default::default()
    };
    (config, dir)
}

// Engine construction goes through ProviderFactory; a dummy key is fine because
// client construction never contacts the network.
fn with_dummy_provider_key() {
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-not-real");
    }
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
    servers.insert("mock".to_string(), McpServerConfig::stdio(command, Vec::new()));
    config.mcp = McpConfig { enabled: true, servers };
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

async fn seed_test_engine_history(engine: &AgentEngine) -> (Vec<rig::message::Message>, Vec<u8>) {
    let sid = &engine.session_manager.session_id;
    let msgs = vec![
        rig::message::Message::user("remember this line"),
        rig::message::Message::assistant("recorded"),
    ];
    engine.session_manager.append(sid, msgs).await.unwrap();
    let history = engine.session_manager.load(sid).await.unwrap();
    let jsonl = engine.config.sessions_dir.join(format!("{sid}.jsonl"));
    let bytes = std::fs::read(&jsonl).unwrap();
    (history, bytes)
}

fn assert_rebuilt_meta(rebuilt: &AgentEngine, sid: &str, tool_count: usize) {
    assert_eq!(
        (rebuilt.config.max_turns, rebuilt.session_manager.session_id.as_str()),
        (7, sid)
    );
    assert_eq!(rebuilt.tool_names().len(), tool_count);
}

async fn assert_rebuilt_storage(rebuilt: &AgentEngine, sid: &str, history: &[rig::message::Message], jsonl: &[u8]) {
    assert_eq!(rebuilt.session_manager.load(sid).await.unwrap(), history);
    assert_eq!(
        std::fs::read(rebuilt.config.sessions_dir.join(format!("{sid}.jsonl"))).unwrap(),
        jsonl
    );
}

#[tokio::test]
async fn rebuild_preserves_session_history_and_reattaches_tools() {
    with_dummy_provider_key();
    let (config, dir) = test_config("continuity");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();
    let tools = crate::tools::build_builtin_tools(&base_dir, &config).unwrap();

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir.clone())
        .tools(tools.clone())
        .build()
        .await
        .unwrap();
    assert_eq!(engine.tool_names().len(), tools.len());

    let session_id = engine.session_manager.session_id.clone();
    let (history_before, jsonl_before) = seed_test_engine_history(&engine).await;

    let mut new_config = config.clone();
    new_config.max_turns = 7;
    let rebuilt = engine.rebuild(new_config, auth_store.clone()).await.unwrap();

    assert_rebuilt_meta(&rebuilt, &session_id, tools.len());
    assert_rebuilt_storage(&rebuilt, &session_id, &history_before, &jsonl_before).await;
    std::fs::remove_dir_all(dir).unwrap();
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
    with_dummy_provider_key();
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

    std::fs::remove_dir_all(dir).unwrap();
}

/// Repeated reloads stay at one live MCP child per configured server.
#[cfg(unix)]
#[tokio::test]
async fn repeated_rebuilds_do_not_leak_mcp_children() {
    with_dummy_provider_key();
    let (config, dir) = test_config("mcp_no_leak");
    let workspace = dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (script_command, script_pattern) = mock_mcp_server(&workspace);
    let config = with_mock_mcp(config, script_command);

    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(workspace.clone())
        .build()
        .await
        .unwrap();

    let mut current = engine;
    for _ in 0..3 {
        current = current.rebuild(config.clone(), auth_store.clone()).await.unwrap();
        wait_for_server_count(&script_pattern, 1).await;
    }

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn builder_attaches_dynamic_plugin_tools() {
    with_dummy_provider_key();
    let (config, dir) = test_config("plugin_tools");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();

    let custom_tool = rig::tool::DynamicTool::new(
        "generate_image",
        "Generate image tool",
        serde_json::json!({
            "type": "object",
            "properties": { "prompt": { "type": "string" } },
            "required": ["prompt"]
        }),
        |_ctx, _args| Box::pin(async { Ok(rig::tool::ToolOutput::text("image.png")) }),
    );

    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir)
        .add_tool(custom_tool)
        .build()
        .await
        .unwrap();

    assert!(engine.tool_names().contains(&"generate_image".to_string()));
    assert!(engine.tool_names().contains(&"read".to_string()));
    assert!(engine.tool_names().contains(&"bash".to_string()));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn refresh_quota_non_antigravity_is_noop() {
    with_dummy_provider_key();
    let (config, dir) = test_config("quota_noop");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(std::env::temp_dir())
        .build()
        .await
        .unwrap();

    assert_eq!(engine.quota_display(), None);
    engine.refresh_quota().await;
    assert_eq!(engine.quota_display(), None);

    std::fs::remove_dir_all(dir).unwrap();
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
        rig::test_utils::MockCompletionModel::default(),
        crate::engine::eval::mock::MockEngineConfig {
            base_dir: &dir,
            app_config: config,
            session_manager: None,
            built_in_tools: None,
        },
    );

    engine.refresh_quota().await;
    assert_eq!(engine.quota_display(), None);

    std::fs::remove_dir_all(dir).unwrap();
}

fn mock_quota_engine(dir: &std::path::Path, provider: &str, model: &str) -> crate::engine::AgentEngine {
    crate::engine::eval::mock::mock_engine(
        rig::test_utils::MockCompletionModel::default(),
        crate::engine::eval::mock::MockEngineConfig {
            base_dir: dir,
            app_config: Config {
                provider: provider.to_string(),
                model: model.to_string(),
                ..Config::default()
            },
            session_manager: None,
            built_in_tools: None,
        },
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
    std::fs::remove_dir_all(dir).unwrap();
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
    engine.config.provider = "openai-chatgpt".to_string();
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
    std::fs::remove_dir_all(dir).unwrap();
}

fn populate_ollama_model_store(config_dir: &std::path::Path) {
    let mut store = crate::provider::ModelStore::load(config_dir.join("models-store.json"));
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
}

#[tokio::test]
async fn context_limit_resolves_ollama_cloud_model_from_model_store() {
    let (config, dir) = test_config("ctx_ollama_cloud");
    populate_ollama_model_store(&config.config_dir);
    let config = Config {
        provider: "ollama-cloud".to_string(),
        model: "glm-5.3-flash".to_string(),
        ..config
    };
    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store.set_key("ollama-cloud", "test-key-not-real").unwrap();
    let engine = builder::AgentEngineBuilder::new(config.clone(), auth_store)
        .base_dir(dir.clone())
        .build()
        .await
        .unwrap();
    assert_eq!(engine.context_limit(), Some(1_048_576));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn explicit_model_prevents_silent_fallback_to_configured_provider() {
    let (config, dir) = test_config("no_fallback_explicit");
    let mut auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    auth_store.set_key("deepseek", "dummy-deepseek-key").unwrap();

    let config = Config {
        provider: "nonexistent-provider".to_string(),
        model: "nonexistent-model".to_string(),
        default_model: Some("nonexistent-model".to_string()),
        default_provider: Some("nonexistent-provider".to_string()),
        ..config
    };

    let result = builder::AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir.clone())
        .build()
        .await;

    assert!(
        result.is_err(),
        "Must not fall back to deepseek when explicit model is set"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn switch_model_updates_model_and_preserves_tools() {
    with_dummy_provider_key();
    let (config, dir) = test_config("switch_model");
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let base_dir = std::env::temp_dir();
    let tools = crate::tools::build_builtin_tools(&base_dir, &config).unwrap();
    let initial_tools_count = tools.len();

    let mut engine = builder::AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .base_dir(base_dir.clone())
        .tools(tools)
        .build()
        .await
        .unwrap();

    assert_eq!(engine.config.model, config.model);
    assert_eq!(engine.tool_names().len(), initial_tools_count);

    engine
        .switch_model("claude-3-5-haiku-20241022", "anthropic")
        .await
        .unwrap();

    assert_eq!(engine.config.model, "claude-3-5-haiku-20241022");
    assert_eq!(engine.config.provider, "anthropic");
    assert_eq!(engine.tool_names().len(), initial_tools_count);
    assert_eq!(engine.context_limit(), Some(200_000));

    std::fs::remove_dir_all(dir).unwrap();
}
