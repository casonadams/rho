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
    let marker = std::env::temp_dir().join(format!("sequential_marker_{}", uuid::Uuid::new_v4()));
    let model = sequential_mutation_model(&marker);
    let config = Config {
        permission: crate::config::PermissionConfig { enabled: false },
        ..Config::default()
    };
    let engine = test_engine(model, config);
    engine
        .run_turn(request("run"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(tokio::fs::read_to_string(&marker).await.unwrap(), "12");
    let _ = tokio::fs::remove_file(marker).await;
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

async fn assert_reopened_session_empty(sm: &SessionManager) {
    assert!(sm.load_messages().await.unwrap().is_empty());
    let reopened = SessionManager::new(sm.file_path.parent().unwrap(), Some(&sm.session_id)).unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_tool_does_not_persist_result() {
    let engine = test_engine(Arc::new(MockAdapter::default()), Config::default());
    let pres = presenter(&TerminalRenderer::default());
    let sink = engine.create_approval_sink(&pres);
    sink.tool_start("bash", &serde_json::json!({"command": "sleep 10"}));
    engine.record_cancellation("interrupted by user").await.unwrap();

    assert_cancellation_events(&engine.session_manager).await;
    assert_reopened_session_empty(&engine.session_manager).await;
}
