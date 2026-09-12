#![cfg(unix)]

use rho::config::{Config, PermissionConfig};
use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter, UiEvent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perm_test_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct PermFixture {
    engine: rho::engine::AgentEngine,
    recording: RecordingSink,
    presenter: Arc<StructuredPresenter>,
}

fn make_perm_engine(workspace: &std::path::Path, model: MockCompletionModel, config: Config) -> PermFixture {
    let built_in_tools = rho_engine::tools::build_builtin_tools(workspace, &config).ok();
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
    PermFixture {
        engine,
        presenter: Arc::new(StructuredPresenter::recording(recording.clone())),
        recording,
    }
}

fn make_perm_model(cmd: &str, final_text: &'static str) -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "bash", json!({"command": cmd})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text(final_text),
            final_event(rig::completion::Usage::new()),
        ],
    ])
}

#[tokio::test]
async fn test_permission_headless_fails_closed() {
    let workspace = temp_workspace();
    let model = make_perm_model("touch forbidden.txt", "saw denial");
    let config = Config {
        permission: PermissionConfig { enabled: true },
        ..Config::default()
    };
    let f = make_perm_engine(&workspace, model.clone(), config);

    let output = f
        .engine
        .run_turn(TurnRequest::new("run forbidden command"), f.presenter)
        .await
        .unwrap();
    assert_eq!(output.final_text, "saw denial");
    assert!(format!("{:?}", model.requests()[1].chat_history).contains("cannot prompt in headless mode"));
    assert!(
        !f.recording
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::ToolStarted { .. }))
    );
}

#[tokio::test]
async fn test_permission_disabled_allows_execution() {
    let workspace = temp_workspace();
    let model = make_perm_model("touch allowed.txt", "done creating");
    let config = Config {
        permission: PermissionConfig { enabled: false },
        ..Config::default()
    };
    let f = make_perm_engine(&workspace, model, config);

    let output = f
        .engine
        .run_turn(TurnRequest::new("touch file"), f.presenter)
        .await
        .unwrap();
    assert_eq!(output.final_text, "done creating");
    assert!(
        f.recording
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::ToolStarted { .. }))
    );
}

#[tokio::test]
async fn test_permission_multiline_bash_command_fails_closed_in_headless() {
    let workspace = temp_workspace();
    let multiline_cmd = "for i in {1..5}; do\n  echo $i\ndone";
    let model = make_perm_model(multiline_cmd, "saw headless denial");
    let config = Config {
        permission: PermissionConfig { enabled: true },
        ..Config::default()
    };
    let f = make_perm_engine(&workspace, model.clone(), config);

    let output = f
        .engine
        .run_turn(TurnRequest::new("run multiline"), f.presenter)
        .await
        .unwrap();
    assert_eq!(output.final_text, "saw headless denial");
    assert!(format!("{:?}", model.requests()[1].chat_history).contains("cannot prompt in headless mode"));
}
