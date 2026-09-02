use super::helpers::{final_event, presenter, request, test_engine};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::runner::{RunStatus, TerminalApprovalSink, TerminalSinkConfig, map_completion_error, redact_text};
use crate::error::AppError;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rig::completion::{FinishReason, Usage};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[tokio::test]
async fn normalized_usage_is_exposed_when_available() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 4,
        total_tokens: 14,
        cached_input_tokens: 3,
        cache_creation_input_tokens: 2,
        tool_use_prompt_tokens: 1,
        reasoning_tokens: 2,
    };
    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("done"), final_event(usage)]]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.usage, Some(usage.into()));
    assert!(output.metrics.usage_available);
    assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(3));
    assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(2));
    assert_eq!(engine.context_usage_display(), "15/200k (0%)");
}

#[tokio::test]
async fn content_filter_finish_is_distinct() {
    let final_record =
        rig::streaming::StreamFinal::new("mock", Usage::new()).with_finish_reason(FinishReason::ContentFilter);
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("filtered partial"),
        MockStreamEvent::FinalResponse(final_record),
    ]]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_eq!(output.status, RunStatus::ContentFiltered);
}

#[tokio::test]
async fn provider_stream_failures_do_not_expose_upstream_details() {
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("authorization: Bearer credential-sentinel")]]);
    let engine = test_engine(model, Config::default());
    let error = engine
        .run_turn(request("prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("Model provider request failed"));
    assert!(!error.contains("credential-sentinel"));
    assert!(!error.contains("Bearer"));
    let persisted = std::fs::read_to_string(&engine.session_manager.file_path).unwrap();
    assert!(!persisted.contains("credential-sentinel"));
    assert!(!persisted.contains("Bearer"));
}

#[tokio::test]
async fn explicit_output_limit_and_max_turn_budget_reach_rig() {
    let config = Config {
        max_output_tokens: Some(321),
        max_turns: 1,
        ..Config::default()
    };
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(model.clone(), config);
    let error = engine
        .run_turn(request("read"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    assert_eq!(model.requests()[0].max_tokens, Some(321));
}

#[test]
fn provider_error_mapping_redacts_sensitive_bodies() {
    let error = rig::completion::CompletionError::from_http_response(
        reqwest::StatusCode::UNAUTHORIZED,
        "authorization: Bearer credential-sentinel",
    );
    let mapped = map_completion_error(error).to_string();
    assert!(mapped.contains("401"));
    assert!(!mapped.contains("credential-sentinel"));
    assert!(!mapped.contains("Bearer"));
}

#[test]
fn terminal_sink_redacts_secret_tool_arguments_and_results() {
    let dir = std::env::temp_dir().join(format!("sink_secret_{}", uuid::Uuid::new_v4()));
    let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
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
    let completed = sink.completed();
    assert_eq!(completed.len(), 1);
    assert!(!completed[0].arguments.to_string().contains("credential-sentinel"));
    assert!(!completed[0].output.contains("credential-sentinel"));
    assert!(completed[0].output.contains("[REDACTED]"));

    let mut displayed = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Output(OutputEvent::Text(text)) => {
                displayed.push_str(&text);
            }
            UiEvent::Transcript(item) => {
                displayed.push_str(&crate::ui::interactive::render_transcript_item(
                    crate::ui::interactive::TranscriptRenderInput {
                        item: &item,
                        theme: &renderer.theme,
                        width: 80,
                        tools_expanded: false,
                    },
                ));
            }
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert!(!displayed.contains("credential-sentinel"));
    assert!(displayed.contains("[REDACTED]"));
}

#[test]
fn cancellation_reason_is_redacted() {
    assert_eq!(
        redact_text("access_token=credential-sentinel"),
        "sensitive upstream detail redacted"
    );
    assert_eq!(redact_text("operator stop"), "operator stop");
}

#[test]
fn auth_store_type_remains_constructible_for_public_engine_api() {
    let _ = AuthStore::default();
}
