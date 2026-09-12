#![cfg(unix)]

use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use rho::engine::runner::TurnRequest;
use rho_harness_core::config::Config;
use rho_harness_core::presentation::{RecordingSink, StructuredPresenter};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hook_integration_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_hook_script(workspace: &Path, event_name: &str, body: &str) {
    let hooks_dir = workspace.join(".rho").join("hooks");
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

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "read", json!({"path": "foo.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("should not reach here"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

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
    let result = engine.run_turn(TurnRequest::new("trigger tool"), presenter).await;

    match result {
        Err(rho_harness_core::error::AppError::Cancelled(reason)) => {
            assert!(reason.contains("blocked by security policy"));
        }
        other => panic!("expected Cancelled error, got {other:?}"),
    }

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

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "read", json!({"path": "original.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("done reading"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

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

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "read", json!({"path": "file.txt"})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("handled skip"),
            final_event(rig::completion::Usage::new()),
        ],
    ]);

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
