#![cfg(unix)]

use rho::config::{Config, PermissionConfig, PluginConfig};
use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter, UiEvent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perm_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_permission_headless_fails_closed() {
    let workspace = temp_workspace();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "bash", json!({"command": "touch forbidden.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("saw denial"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

    let config = Config {
        permission: PermissionConfig { enabled: true },
        ..Config::default()
    };
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();

    let engine = mock_engine(
        model.clone(),
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("run forbidden command"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "saw denial");

    let requests = model.requests();
    assert_eq!(requests.len(), 2);
    let history = format!("{:?}", requests[1].chat_history);
    assert!(history.contains("cannot prompt in headless mode"));

    let events = recording.events();
    let ran_tool = events.iter().any(|e| matches!(e, UiEvent::ToolStarted { .. }));
    assert!(!ran_tool, "Tool must not start when permission fails closed");
}

#[tokio::test]
async fn test_permission_disabled_allows_execution() {
    let workspace = temp_workspace();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "bash", json!({"command": "touch allowed.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("done creating"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

    let config = Config {
        permission: PermissionConfig { enabled: false },
        ..Config::default()
    };
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();

    let engine = mock_engine(
        model.clone(),
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("touch file"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "done creating");

    let events = recording.events();
    let ran_tool = events.iter().any(|e| matches!(e, UiEvent::ToolStarted { .. }));
    assert!(ran_tool, "Tool must execute when permission is disabled");
}

#[tokio::test]
async fn test_permission_plugin_override_disables_builtin_hook() {
    let workspace = temp_workspace();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "bash", json!({"command": "touch plugin_override.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("plugin allowed"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

    let mut plugins = std::collections::BTreeMap::new();
    plugins.insert(
        "permission".to_string(),
        PluginConfig {
            enabled: true,
            ..PluginConfig::default()
        },
    );

    let config = Config {
        permission: PermissionConfig { enabled: true },
        plugins,
        ..Config::default()
    };
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();

    let engine = mock_engine(
        model.clone(),
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("run with plugin override"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "plugin allowed");

    let events = recording.events();
    let ran_tool = events.iter().any(|e| matches!(e, UiEvent::ToolStarted { .. }));
    assert!(
        ran_tool,
        "Tool must execute because built-in permission hook yields to external plugin"
    );
}
