#[cfg(unix)]
use super::super::helpers::{final_event, presenter, request, test_engine};
#[cfg(unix)]
use crate::config::Config;
#[cfg(unix)]
use crate::session::SessionManager;
#[cfg(unix)]
use crate::ui::TerminalRenderer;
#[cfg(unix)]
use rig::completion::Usage;
#[cfg(unix)]
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[cfg(unix)]
#[tokio::test]
async fn mutating_tools_execute_sequentially() {
    let marker = std::env::temp_dir().join(format!("sequential_marker_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call(
                "call-1",
                "bash",
                serde_json::json!({"command": format!("sleep 0.05; printf 1 >> {}", marker.display())}),
            ),
            MockStreamEvent::tool_call(
                "call-2",
                "bash",
                serde_json::json!({"command": format!("printf 2 >> {}", marker.display())}),
            ),
            final_event(Usage::new()),
        ],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(
        model,
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    engine
        .run_turn(request("run"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(tokio::fs::read_to_string(&marker).await.unwrap(), "12");
    let _ = tokio::fs::remove_file(marker).await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_tool_run_persists_no_incomplete_result() {
    let marker = std::env::temp_dir().join(format!("cancel_marker_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "call-1",
            "bash",
            serde_json::json!({"command": format!("sleep 2; touch {}", marker.display())}),
        ),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(
        model,
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        engine.run_turn(request("run"), presenter(&TerminalRenderer::default())),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert!(result.is_err());
    engine.record_cancellation("test interrupt").await.unwrap();
    assert!(!marker.exists());
    let events = engine.session_manager.load_events().await.unwrap();
    assert!(
        !events
            .iter()
            .any(|event| event.kind == crate::session::SessionEventKind::ToolResult)
    );
    assert!(
        events
            .iter()
            .any(|event| event.kind == crate::session::SessionEventKind::Cancellation)
    );
    let summary = events
        .iter()
        .find(|event| event.kind == crate::session::SessionEventKind::RunSummary)
        .unwrap();
    assert_eq!(summary.payload["terminal_status"], "cancelled");
    assert!(engine.session_manager.load_messages().await.unwrap().is_empty());
    let reopened = SessionManager::new(
        engine.session_manager.file_path.parent().unwrap(),
        Some(&engine.session_manager.session_id),
    )
    .unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
}
