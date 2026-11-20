use super::super::helpers::{presenter, request, test_engine};
use crate::config::Config;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, TranscriptItem, UiEvent};
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use std::sync::Arc;

fn shown_tool_outputs(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Vec<String> {
    let mut outputs = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Transcript(TranscriptItem::Tool(line)) = event {
            outputs.push(line.output);
        }
    }
    outputs
}

#[cfg(unix)]
#[tokio::test]
async fn the_model_sees_plain_bash_output_while_the_ui_keeps_its_colors() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call-1",
        ToolFunction::new(
            "bash",
            serde_json::json!({"command": "printf '\\033[31mred\\033[0m\\n'; printf '\\033[1;32mgreen\\033[0m\\n'"}),
        ),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("done".to_string()))]);
    let engine = test_engine(model.clone(), Config::default());
    let (ui, mut events) = InteractiveUi::channel();

    engine
        .run_turn(request("show colors"), presenter(&TerminalRenderer::with_ui(ui)))
        .await
        .unwrap();

    let sent_to_model = serde_json::to_string(&model.requests()[1].messages).unwrap();
    assert!(sent_to_model.contains("red\\ngreen"), "{sent_to_model}");
    assert!(
        !sent_to_model.contains("\\u001b"),
        "the model got escape codes: {sent_to_model}"
    );
    assert_eq!(
        shown_tool_outputs(&mut events),
        ["\u{1b}[31mred\u{1b}[0m\n\u{1b}[1;32mgreen\u{1b}[0m"]
    );
    let saved = serde_json::to_string(&engine.session_manager.load_messages().await.unwrap()).unwrap();
    assert!(
        !saved.contains("\\u001b"),
        "the saved history has escape codes: {saved}"
    );
}

#[tokio::test]
async fn a_turn_ends_while_another_sessions_background_job_runs_and_never_receives_its_result() {
    use crate::engine::runner::turn::types::{drain_background_notifications, enqueue_background_notification};
    use rho_engine::tools::jobs::{JobManager, JobStatus};
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::Text("all done".to_string()))]);
    let engine = test_engine(model.clone(), Config::default());
    let jobs = JobManager::global();
    let elsewhere = jobs.register("e2e-other-session", "e2e-other-job".into(), "sleep 1000".into());
    enqueue_background_notification("e2e-other-session", "e2e-other-secret-result".into());

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.run_turn(request("finish up"), presenter(&TerminalRenderer::default())),
    )
    .await
    .expect("the turn must not wait for a job that belongs to another session")
    .unwrap();

    let sent_to_model = serde_json::to_string(&model.requests()[0].messages).unwrap();
    assert!(!sent_to_model.contains("e2e-other-secret-result"), "{sent_to_model}");
    assert!(!elsewhere.is_cancelled());
    assert_eq!(
        drain_background_notifications("e2e-other-session"),
        ["e2e-other-secret-result"]
    );
    jobs.update_status(
        "e2e-other-job",
        JobStatus::Failed {
            error: "test over".into(),
        },
    );
}
