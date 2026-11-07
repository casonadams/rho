#![cfg(unix)]

use crate::{temp_dir, write_script};
use rho::config::Config;
use rho::engine::eval::mock::{MockEngineConfig, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter};
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use serde_json::json;
use std::sync::Arc;

fn write_hook_script(hooks_dir: &std::path::Path, event_name: &str, body: &str) {
    std::fs::create_dir_all(hooks_dir).unwrap();
    write_script(&hooks_dir.join(event_name), &format!("#!/bin/sh\n{body}\n"));
}

async fn run_read_turn(
    workspace: &std::path::Path,
    read_path: &str,
    final_text: &str,
) -> rho_harness_core::error::Result<rho_engine::engine::runner::TurnOutput> {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("read", json!({"path": read_path})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text(final_text.to_string()))]);

    let config = Config::default();
    let built_in_tools = rho_engine::tools::build_builtin_tools(workspace, &config, None).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );

    let recording = RecordingSink::default();
    let presenter = Arc::new(StructuredPresenter::new(Arc::new(recording)));
    engine.run_turn(TurnRequest::new("read file"), presenter).await
}

async fn run_hooked_read(
    hooks_dir: &str,
    body: &str,
    files: &[(&str, &str)],
    read_path: &str,
    final_text: &str,
) -> rho_harness_core::error::Result<rho_engine::engine::runner::TurnOutput> {
    let workspace = temp_dir("hook_integration_test");
    for (name, contents) in files {
        std::fs::write(workspace.join(name), contents).unwrap();
    }
    write_hook_script(&workspace.join(hooks_dir), "on_tool_call", body);
    let result = run_read_turn(&workspace, read_path, final_text).await;
    let _ = std::fs::remove_dir_all(&workspace);
    result
}

#[tokio::test]
async fn test_hook_stops_agent_mid_cycle() {
    let result = run_hooked_read(
        ".agents/hooks",
        r#"echo '{"action":"stop","reason":"blocked by security policy"}'"#,
        &[],
        "foo.txt",
        "should not reach here",
    )
    .await;
    assert!(result.is_err() || result.unwrap().final_text == "should not reach here");
}

#[tokio::test]
async fn test_hook_rewrites_tool_args() {
    let output = run_hooked_read(
        ".agents/hooks",
        r#"echo '{"action":"rewrite_args","args":{"path":"rewritten.txt"}}'"#,
        &[("rewritten.txt", "content of rewritten file")],
        "original.txt",
        "done reading",
    )
    .await
    .unwrap();

    assert_eq!(output.tool_calls_count, 1);
    assert_eq!(output.final_text, "done reading");
}

#[tokio::test]
async fn test_hooked_read_turns_finish_with_expected_text() {
    let cases = [
        (
            ".agents/hooks",
            r#"echo '{"action":"skip","reason":"manual skip"}'"#,
            "file.txt",
            "handled skip",
        ),
        (
            ".rho/hooks",
            r#"echo '{"action":"stop","reason":"legacy hook triggered"}'"#,
            "foo.txt",
            "finished normally",
        ),
    ];
    for (hooks_dir, body, read_path, final_text) in cases {
        let output = run_hooked_read(hooks_dir, body, &[("foo.txt", "hello")], read_path, final_text)
            .await
            .unwrap();
        assert_eq!(output.final_text, final_text);
    }
}
