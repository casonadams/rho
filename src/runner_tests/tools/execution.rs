#[cfg(unix)]
use super::super::helpers::{presenter, request, test_engine};
#[cfg(unix)]
use crate::config::Config;
#[cfg(unix)]
use crate::session::SessionManager;
#[cfg(unix)]
use crate::ui::TerminalRenderer;
#[cfg(unix)]
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
#[cfg(unix)]
use rho_harness_core::model::{ToolCall, ToolFunction};
#[cfg(unix)]
use std::sync::Arc;

#[cfg(unix)]
fn sequential_mutation_model(marker: &std::path::Path) -> Arc<MockAdapter> {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![
        Ok(ModelStreamEvent::ToolCall(ToolCall::new(
            "call-1",
            ToolFunction::new(
                "bash",
                serde_json::json!({"command": format!("sleep 0.05; printf 1 >> {}", marker.display())}),
            ),
        ))),
        Ok(ModelStreamEvent::ToolCall(ToolCall::new(
            "call-2",
            ToolFunction::new(
                "bash",
                serde_json::json!({"command": format!("printf 2 >> {}", marker.display())}),
            ),
        ))),
    ]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("done".to_string()))]);
    model
}

#[cfg(unix)]
#[tokio::test]
async fn mutating_tools_execute_sequentially() {
    let dir = rho_test_support::TestDir::new();
    let marker = dir.join("sequential_marker");
    let model = sequential_mutation_model(&marker);
    let engine = test_engine(model, Config::default());
    engine
        .run_turn(request("run"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(tokio::fs::read_to_string(&marker).await.unwrap(), "12");
}

#[cfg(unix)]
async fn assert_cancellation_events(sm: &SessionManager) {
    let events = sm.load_events().await.unwrap();
    assert!(
        !events
            .iter()
            .any(|e| e.kind == crate::session::SessionEventKind::ToolResult)
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == crate::session::SessionEventKind::Cancellation)
    );
    let summary = events
        .iter()
        .find(|e| e.kind == crate::session::SessionEventKind::RunSummary)
        .unwrap();
    assert_eq!(summary.payload["terminal_status"], "cancelled");
}

#[tokio::test]
async fn cancelled_tool_does_not_persist_result() {
    use crate::engine::runner::{CancellationSignal, TurnRequest};
    use std::time::Duration;

    let dir = rho_test_support::TestDir::new();
    let started = dir.join("tool_started");
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new(
            "bash",
            serde_json::json!({"command": format!("touch {}; sleep 30", started.display())}),
        ),
    )))]);
    let engine = test_engine(model, Config::default());
    let signal = CancellationSignal::default();
    let turn_request = TurnRequest {
        prompt: "run the slow tool",
        cancellation: Some(&signal),
        steering: None,
    };
    let presenter = presenter(&TerminalRenderer::default());

    let canceller = async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        signal.cancel();
    };
    let began = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            _ = engine.run_turn(turn_request, presenter) => panic!("the tool finished before it was cancelled"),
            () = canceller => {}
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the tool never started"));
    engine.record_cancellation("operator interrupt").await.unwrap();

    assert!(began.elapsed() < Duration::from_secs(2));
    assert_cancellation_events(&engine.session_manager).await;
    assert!(engine.session_manager.load_messages().await.unwrap().is_empty());
    let reopened = SessionManager::new(
        engine.session_manager.file_path.parent().unwrap(),
        Some(&engine.session_manager.session_id),
    )
    .unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
}
