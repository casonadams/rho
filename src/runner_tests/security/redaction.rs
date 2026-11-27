use super::super::helpers::{drain_ui_events, presenter, request, test_engine};
use crate::config::Config;
use crate::engine::runner::{TerminalApprovalSink, TerminalSinkConfig, redact_text};
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rho_engine::provider::adapter::MockAdapter;
use std::sync::Arc;

#[tokio::test]
async fn provider_stream_failures_do_not_expose_upstream_details() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Err(rho_harness_core::error::AppError::Provider(redact_text(
        "authorization: Bearer credential-sentinel",
    )))]);
    let engine = test_engine(model, Config::default());
    let error = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err()
        .to_string();

    assert!(!error.contains("credential-sentinel"));
    assert!(!error.contains("Bearer"));
    let persisted = std::fs::read_to_string(&engine.session_manager.file_path).unwrap();
    assert!(!persisted.contains("credential-sentinel"));
    assert!(!persisted.contains("Bearer"));
}

fn collect_displayed_events(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    renderer: &TerminalRenderer,
) -> String {
    drain_ui_events(events, |event| match event {
        UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) => Some(text),
        UiEvent::Transcript(item) => Some(crate::ui::interactive::render_transcript_item(
            crate::ui::interactive::TranscriptRenderInput {
                item: &item,
                theme: &renderer.theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            },
        )),
        UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        _ => None,
    })
    .concat()
}

fn assert_redacted(completed: &[rho_engine::engine::runner::CompletedTool], displayed: &str, secret: &str) {
    assert_eq!(completed.len(), 1);
    assert!(!completed[0].arguments.to_string().contains(secret) && !completed[0].output.contains(secret));
    assert!(completed[0].output.contains("[REDACTED]"));
    let plain = crate::ui::block::strip_ansi(displayed);
    assert!(!displayed.contains(secret) && !plain.contains(secret) && plain.contains("[REDACTED]"));
}

#[test]
fn terminal_sink_redacts_secret_tool_arguments_and_results() {
    let dir = rho_test_support::TestDir::new();
    let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    let args = serde_json::json!({"path":"credential-sentinel"});
    sink.tool_start("read", &args);
    sink.tool_finished(rho_engine::engine::runner::ToolFinishDetails {
        name: "read",
        arguments: &args,
        output: "credential-sentinel",
        is_error: true,
    });
    let displayed = collect_displayed_events(&mut events, &renderer);
    assert_redacted(&sink.completed(), &displayed, "credential-sentinel");
}

#[test]
fn cancellation_reason_is_redacted() {
    assert_eq!(
        redact_text("access_token=credential-sentinel"),
        "sensitive upstream detail redacted"
    );
    assert_eq!(redact_text("operator stop"), "operator stop");
}
