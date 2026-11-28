#![cfg(unix)]

use crate::temp_dir;
use rho::config::{Config, PermissionConfig};
use rho::engine::eval::mock::{MockEngineConfig, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::StructuredPresenter;
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use serde_json::json;
use std::sync::Arc;

struct PermFixture {
    engine: rho::engine::AgentEngine,
    presenter: Arc<StructuredPresenter>,
}

fn make_perm_engine(workspace: &std::path::Path, model: Arc<MockAdapter>, config: Config) -> PermFixture {
    let built_in_tools = rho_engine::tools::build_builtin_tools(workspace, &config, None).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            built_in_tools,
            ..MockEngineConfig::new(workspace, config)
        },
    );
    let recording = rho::presentation::RecordingSink::default();
    PermFixture {
        engine,
        presenter: Arc::new(StructuredPresenter::recording(recording)),
    }
}

fn make_perm_model(cmd: &str, final_text: &'static str) -> Arc<MockAdapter> {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("bash", json!({"command": cmd})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text(final_text.to_string()))]);
    model
}

/// RHO_HOME pointed at a scratch dir, so permission rules come only from the test.
fn isolate_perm_env() -> (rho_test_support::EnvVarGuard, rho_test_support::TestDir) {
    let dir = rho_test_support::TestDir::with_prefix("perm_global_");
    (
        rho_test_support::EnvVarGuard::set("RHO_HOME", dir.to_str().unwrap()),
        dir,
    )
}

struct HeadlessRun {
    result: rho_harness_core::error::Result<rho_engine::engine::runner::TurnOutput>,
    model: Arc<MockAdapter>,
    workspace: rho_test_support::TestDir,
}

impl HeadlessRun {
    fn tool_result_text(&self) -> String {
        use rho_harness_core::model::{ChatMessage, UserContent};
        let requests = self.model.requests();
        let last = requests.last().expect("the model was called after the tool");
        last.messages
            .iter()
            .filter_map(|m| match m {
                ChatMessage::User { content } => Some(content),
                _ => None,
            })
            .flatten()
            .filter_map(|c| match c {
                UserContent::ToolResult(r) => Some(r.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn run_headless_turn(
    rules: Option<&str>,
    command_for: impl FnOnce(&std::path::Path) -> String,
    final_text: &'static str,
    prompt: &str,
    setup: impl FnOnce(&std::path::Path),
) -> HeadlessRun {
    let _env = isolate_perm_env();
    let ws = temp_dir("perm_test");
    if let Some(rules) = rules {
        let perm_dir = ws.join(".rho");
        std::fs::create_dir_all(&perm_dir).unwrap();
        std::fs::write(perm_dir.join("permission.toml"), rules).unwrap();
    }
    setup(&ws);
    let config = Config {
        permission: PermissionConfig::default(),
        ..Config::default()
    };
    let model = make_perm_model(&command_for(&ws), final_text);
    let fix = make_perm_engine(&ws, Arc::clone(&model), config);
    let result = fix.engine.run_turn(TurnRequest::new(prompt), fix.presenter).await;
    HeadlessRun {
        result,
        model,
        workspace: ws,
    }
}

#[tokio::test]
async fn permission_explicit_allow_runs_silently_in_headless() {
    let run = run_headless_turn(
        Some("[permission.bash]\n\"echo *\" = \"allow\"\n"),
        |_| "echo hello_headless".to_string(),
        "done echoing",
        "run echo",
        |_| {},
    )
    .await;

    let tool_text = run.tool_result_text();
    let result = run.result;
    assert_eq!(result.unwrap().final_text, "done echoing");
    assert!(tool_text.contains("hello_headless"));
}

#[tokio::test]
async fn permission_explicit_deny_blocks_the_command_in_headless() {
    let run = run_headless_turn(
        Some("[permission.bash]\n\"rm *\" = \"deny\"\n"),
        |ws| format!("rm -rf {}", ws.join("victim").display()),
        "done removing",
        "remove victim",
        |ws| std::fs::write(ws.join("victim"), "keep me").unwrap(),
    )
    .await;

    assert_eq!(
        std::fs::read_to_string(run.workspace.join("victim")).unwrap(),
        "keep me"
    );
    assert!(run.result.is_ok(), "a denied tool call does not abort the turn");
    assert_eq!(
        run.tool_result_text(),
        "Permission denied: denied by permission rule 'bash|rm *'"
    );
}

#[tokio::test]
async fn permission_ask_denies_cleanly_in_headless_mode() {
    let run = run_headless_turn(
        None,
        |ws| format!("touch {}", ws.join("unmatched_file").display()),
        "should not reach here",
        "touch file",
        |_| {},
    )
    .await;

    assert!(!run.workspace.join("unmatched_file").exists());
    assert!(run.result.is_ok(), "a denied tool call does not abort the turn");
    assert!(
        run.tool_result_text().contains("cannot prompt in headless mode"),
        "{}",
        run.tool_result_text()
    );
}
