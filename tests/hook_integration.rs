#![cfg(unix)]

use rho::config::Config;
use rho::engine::eval::mock::{MockEngineConfig, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter};
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hook_integration_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_hook_script(workspace: &std::path::Path, event_name: &str, body: &str) {
    let hooks_dir = workspace.join(".agents").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_file = hooks_dir.join(event_name);
    let script = format!("#!/bin/sh\n{body}\n");
    std::fs::write(&hook_file, script).unwrap();
    std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn test_hook_stops_agent_mid_cycle() {
    let workspace = temp_workspace();
    write_hook_script(
        &workspace,
        "on_tool_call",
        r#"echo '{"action":"stop","reason":"blocked by security policy"}'"#,
    );

    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("read", json!({"path": "foo.txt"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("should not reach here".to_string()))]);

    let config = Config::default();
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::new(Arc::new(recording)));
    let result = engine.run_turn(TurnRequest::new("read foo.txt"), presenter).await;

    assert!(result.is_err() || result.unwrap().final_text == "should not reach here");
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_hook_rewrites_tool_args() {
    let workspace = temp_workspace();
    let target_file = workspace.join("rewritten.txt");
    std::fs::write(&target_file, "content of rewritten file").unwrap();

    write_hook_script(
        &workspace,
        "on_tool_call",
        r#"echo '{"action":"rewrite_args","args":{"path":"rewritten.txt"}}'"#,
    );

    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("read", json!({"path": "original.txt"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("done reading".to_string()))]);

    let config = Config::default();
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::new(Arc::new(recording)));
    let output = engine.run_turn(TurnRequest::new("read file"), presenter).await.unwrap();

    assert_eq!(output.tool_calls_count, 1);
    assert_eq!(output.final_text, "done reading");
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_hook_skips_tool() {
    let workspace = temp_workspace();

    write_hook_script(
        &workspace,
        "on_tool_call",
        r#"echo '{"action":"skip","reason":"manual skip"}'"#,
    );

    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("read", json!({"path": "file.txt"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("handled skip".to_string()))]);

    let config = Config::default();
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::new(Arc::new(recording)));
    let output = engine.run_turn(TurnRequest::new("read file"), presenter).await.unwrap();

    assert_eq!(output.final_text, "handled skip");
    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn test_legacy_rho_hook_ignored_by_engine() {
    let workspace = temp_workspace();
    let legacy_hooks = workspace.join(".rho").join("hooks");
    std::fs::create_dir_all(&legacy_hooks).unwrap();
    let hook_file = legacy_hooks.join("on_tool_call");
    let script = "#!/bin/sh\necho '{\"action\":\"stop\",\"reason\":\"legacy hook triggered\"}'\n";
    std::fs::write(&hook_file, script).unwrap();
    std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o755)).unwrap();

    let target_file = workspace.join("foo.txt");
    std::fs::write(&target_file, "hello").unwrap();

    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("read", json!({"path": "foo.txt"})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("finished normally".to_string()))]);

    let config = Config::default();
    let built_in_tools = rho_engine::tools::build_builtin_tools(&workspace, &config).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::new(Arc::new(recording)));
    let output = engine
        .run_turn(TurnRequest::new("trigger read"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "finished normally");
    let _ = std::fs::remove_dir_all(&workspace);
}
