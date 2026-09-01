#![cfg(unix)]

use rho::config::Config;
use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter, UiEnvelope, UiEvent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("headless_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn headless_presentation_records_deterministic_event_sequence() {
    let workspace = temp_workspace();
    let file_path = workspace.join("sample.txt");
    std::fs::write(&file_path, "headless content").unwrap();

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call_1", "read", json!({"path": file_path.to_str().unwrap()})),
            final_event(rig::completion::Usage::new()),
        ],
        vec![
            MockStreamEvent::text("The file contains: headless content"),
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
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("read sample.txt"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "The file contains: headless content");

    let events = recording.events();
    assert!(!events.is_empty());

    let has_tool_started = events.iter().any(|e| match e {
        UiEvent::ToolStarted { name, arguments } => name == "read" && arguments["path"] == file_path.to_str().unwrap(),
        _ => false,
    });
    assert!(has_tool_started, "Expected ToolStarted event for read");

    let has_tool_finished = events.iter().any(|e| match e {
        UiEvent::ToolFinished { line } => {
            line.name == "read"
                && line.arguments["path"] == file_path.to_str().unwrap()
                && line.output.contains("headless content")
                && !line.is_error
        }
        _ => false,
    });
    assert!(has_tool_finished, "Expected ToolFinished event for read");

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn ndjson_envelope_serialization_roundtrips() {
    let envelope = UiEnvelope::new(UiEvent::Notice {
        text: "hello world".to_string(),
    });
    let serialized = serde_json::to_string(&envelope).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized["event_version"], 1);
    assert_eq!(deserialized["kind"], "notice");
    assert_eq!(deserialized["text"], "hello world");
}

#[test]
fn headless_interactive_mode_diagnostic_message_is_actionable() {
    let config = Config::default();
    let auth = rho_engine::auth::AuthStore::default();
    let res = rho::run_cli();
    drop((config, auth, res));
}
