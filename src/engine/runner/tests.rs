use super::helpers::redact_text;
use super::history::{display_events, map_completion_error};
use super::sink::{TerminalApprovalSink, TerminalSinkConfig};
use super::turn::{RunStatus, TurnRequest};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::engine::runtime::{CodingRuntime, build_coding_agent};
use crate::error::AppError;
use crate::session::SessionManager;
use crate::tools::approval::{ApprovalEventSink, ToolEvent};
use crate::tools::bash_ast::RiskTier;
use crate::tools::policy::ExecutionClass;
use crate::ui::TerminalRenderer;
use rig::agent::ModelHandle;
use rig::completion::{FinishReason, Usage};
use rig::message::{AssistantContent, Message, UserContent};
use rig::streaming::StreamedAssistantContent;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::collections::HashSet;
use std::sync::Mutex;

fn test_engine(model: MockCompletionModel, config: Config) -> AgentEngine {
    let dir = std::env::temp_dir().join(format!("runner_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let session_manager = SessionManager::new(&dir, None).unwrap();
    test_engine_with_session(model, config, session_manager)
}

fn test_engine_with_session(
    model: MockCompletionModel,
    config: Config,
    session_manager: SessionManager,
) -> AgentEngine {
    let base_dir = session_manager.file_path.parent().unwrap();
    let agent = build_coding_agent(
        ModelHandle::new(model),
        &config,
        CodingRuntime {
            base_dir,
            memory: session_manager.clone(),
        },
    )
    .unwrap();
    AgentEngine {
        config,
        session_manager,
        agent,
        last_usage: Mutex::new(None),
        run_tracker: crate::engine::metrics::RunTracker::default(),
    }
}

fn terminal_session() -> SessionManager {
    let dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
    SessionManager::new(&dir, None).unwrap()
}

fn final_event(usage: Usage) -> MockStreamEvent {
    MockStreamEvent::final_response(usage)
}

fn request(prompt: &str) -> TurnRequest<'_> {
    TurnRequest { prompt, intent: None }
}

#[test]
fn renderer_events_preserve_reasoning_text_order_without_duplicates() {
    let mut reasoning_parts = HashSet::new();
    let events = [
        StreamedAssistantContent::ReasoningDelta {
            id: "reasoning-1".to_string(),
            provider_id: None,
            reasoning: "think".to_string(),
        },
        StreamedAssistantContent::Reasoning {
            id: "reasoning-1".to_string(),
            reasoning: rig::message::Reasoning::new("think"),
        },
        StreamedAssistantContent::text("answer"),
    ]
    .into_iter()
    .flat_map(|item| display_events(item, &mut reasoning_parts))
    .collect::<Vec<_>>();

    assert_eq!(
        events,
        [
            super::history::DisplayEvent::Reasoning("think".to_string()),
            super::history::DisplayEvent::Text("answer".to_string())
        ]
    );
}

#[test]
fn visible_stream_clears_spinner_and_hidden_output_resumes_it() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &renderer,
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit_reasoning("think ");
    sink.emit_reasoning("harder");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "think harder");
    assert!(sink.state.lock().unwrap().spinner.is_none());

    sink.emit_text("answer");
    assert!(sink.state.lock().unwrap().reasoning.is_empty());
    assert!(sink.state.lock().unwrap().spinner.is_none());

    sink.resume_model_spinner();
    assert!(sink.state.lock().unwrap().spinner.is_some());
    sink.finish_spinner();
}

#[test]
fn approval_required_holds_spinner_until_granted() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &renderer,
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "rm -rf target" }),
        class: ExecutionClass::ApprovalRequired {
            tier: RiskTier::HighRisk,
            reasons: vec!["destructive".to_string()],
        },
    });
    // No spinner while the approval prompt is on screen.
    assert!(sink.state.lock().unwrap().spinner.is_none());
    assert!(sink.state.lock().unwrap().pending.contains_key("call-1"));

    sink.emit(ToolEvent::ApprovalGranted {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
    });
    // The spinner starts only once execution is actually approved.
    assert!(sink.state.lock().unwrap().spinner.is_some());
}

#[test]
fn approval_denied_leaves_no_spinner() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &renderer,
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "write".to_string(),
        arguments: serde_json::json!({ "path": "/tmp/x", "content": "y" }),
        class: ExecutionClass::ApprovalRequired {
            tier: RiskTier::Mutating,
            reasons: vec!["outside".to_string()],
        },
    });
    sink.emit(ToolEvent::ApprovalDenied {
        internal_call_id: "call-1".to_string(),
        tool_name: "write".to_string(),
    });
    assert!(sink.state.lock().unwrap().spinner.is_none());
}

#[test]
fn reasoning_flushes_before_tool_classification() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &renderer,
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit_reasoning("pondering next step");
    assert_eq!(sink.state.lock().unwrap().reasoning.join(""), "pondering next step");

    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "cargo test" }),
        class: ExecutionClass::ReadOnly,
    });

    assert!(sink.state.lock().unwrap().reasoning.is_empty());
}

#[tokio::test]
async fn final_text_streams_once_and_usage_can_be_unavailable() {
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("final text"), final_event(Usage::new())]]);
    let engine = test_engine(model, Config::default());
    let output = engine
        .run_turn(request("prompt"), &TerminalRenderer::default())
        .await
        .unwrap();
    assert_eq!(output.final_text, "final text");
    assert_eq!(output.usage, None);
    assert_eq!(output.requests, 1);
    assert!(!output.metrics.usage_available);
    assert_eq!(output.metrics.model_turns, 1);
}

#[tokio::test]
async fn two_prompts_receive_prior_canonical_history_exactly_once() {
    let model = MockCompletionModel::from_stream_turns([
        [MockStreamEvent::text("first answer"), final_event(Usage::new())],
        [MockStreamEvent::text("second answer"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    engine
        .run_turn(request("first prompt"), &TerminalRenderer::default())
        .await
        .unwrap();
    engine
        .run_turn(request("second prompt"), &TerminalRenderer::default())
        .await
        .unwrap();

    let second = &model.requests()[1].chat_history;
    let encoded = serde_json::to_string(second).unwrap();
    assert_eq!(second.len(), 4, "{encoded}");
    assert_eq!(encoded.matches("first prompt").count(), 1);
    assert_eq!(encoded.matches("first answer").count(), 1);
    assert_eq!(encoded.matches("second prompt").count(), 1);
}

#[tokio::test]
async fn process_style_reopen_resumes_canonical_history_once() {
    let first_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("persisted answer"),
        final_event(Usage::new()),
    ]]);
    let first = test_engine(first_model, Config::default());
    first
        .run_turn(request("persisted prompt"), &TerminalRenderer::default())
        .await
        .unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("resumed answer"), final_event(Usage::new())]]);
    let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), resumed_store);
    resumed
        .run_turn(request("resume prompt"), &TerminalRenderer::default())
        .await
        .unwrap();

    let history = &resumed_model.requests()[0].chat_history;
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(history.len(), 4, "{encoded}");
    assert_eq!(encoded.matches("persisted prompt").count(), 1);
    assert_eq!(encoded.matches("persisted answer").count(), 1);
}

#[tokio::test]
async fn model_rebuild_preserves_compatible_history_without_duplication() {
    let config = Config {
        provider: "ollama".to_string(),
        model: "first-local-model".to_string(),
        ..Config::default()
    };
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("stored answer"), final_event(Usage::new())]]);
    let engine = test_engine(model, config.clone());
    engine
        .run_turn(request("stored prompt"), &TerminalRenderer::default())
        .await
        .unwrap();
    let id = engine.session_manager.session_id.clone();
    let rebuilt = engine
        .rebuild(
            Config {
                model: "second-local-model".to_string(),
                ..config
            },
            AuthStore::default(),
        )
        .await
        .unwrap();

    assert_eq!(rebuilt.session_manager.session_id, id);
    let encoded = serde_json::to_string(&rebuilt.session_manager.load_messages().await.unwrap()).unwrap();
    assert_eq!(encoded.matches("stored prompt").count(), 1);
    assert_eq!(encoded.matches("stored answer").count(), 1);
}

#[tokio::test]
async fn one_tool_round_preserves_canonical_call_and_one_result() {
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing"})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read"), &TerminalRenderer::default())
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 1);
    let request = &model.requests()[1];
    let assistant_calls = request
        .chat_history
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { content, .. } => Some(
                content
                    .iter()
                    .filter(|content| matches!(content, AssistantContent::ToolCall(_)))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    let results = request
        .chat_history
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter(|content| matches!(content, UserContent::ToolResult(_)))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!((assistant_calls, results), (1, 1));
}

#[tokio::test]
async fn multiple_tool_calls_have_one_correlated_result_each() {
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": "missing-a"})),
            MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path": "missing-b"})),
            final_event(Usage::new()),
        ],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    let output = engine
        .run_turn(request("read both"), &TerminalRenderer::default())
        .await
        .unwrap();

    assert_eq!(output.tool_calls_count, 2);
    assert_eq!(output.tool_failures_count, 2);
    let request = &model.requests()[1];
    let serialized = serde_json::to_value(&request.chat_history).unwrap();
    let calls = serialized.to_string().matches("toolcall").count();
    let results = serialized.to_string().matches("toolresult").count();
    assert_eq!((calls, results), (2, 2));
}

#[tokio::test]
async fn malformed_tool_arguments_are_model_visible_tool_failures() {
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"unexpected": true})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("recovered"), final_event(Usage::new())],
    ]);
    let engine = test_engine(
        model.clone(),
        Config {
            auto_approve: true,
            ..Config::default()
        },
    );
    let output = engine
        .run_turn(request("read"), &TerminalRenderer::default())
        .await
        .unwrap();

    assert_eq!(output.tool_failures_count, 1);
    assert!(format!("{:?}", model.requests()[1]).contains("failed to parse tool arguments"));
}

#[tokio::test]
async fn unknown_tool_calls_fail_without_fallback() {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "unknown", serde_json::json!({})),
        final_event(Usage::new()),
    ]]);
    let engine = test_engine(model, Config::default());
    let error = engine
        .run_turn(request("unknown"), &TerminalRenderer::default())
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::InvalidToolCall(name) if name == "unknown"));
}

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
        .run_turn(request("prompt"), &TerminalRenderer::default())
        .await
        .unwrap();

    assert_eq!(output.usage, Some(usage.into()));
    assert!(output.metrics.usage_available);
    assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(3));
    assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(2));
    assert_eq!(engine.context_usage_display(), "10 input tokens");
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
        .run_turn(request("prompt"), &TerminalRenderer::default())
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
        .run_turn(request("prompt"), &TerminalRenderer::default())
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
        .run_turn(request("read"), &TerminalRenderer::default())
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    assert_eq!(model.requests()[0].max_tokens, Some(321));
}

#[tokio::test]
async fn budget_exhausted_checkpoint_survives_process_resume_and_promotes_once() {
    let first_model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path":"missing-a"})),
            final_event(Usage::new()),
        ],
        [
            MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path":"missing-b"})),
            final_event(Usage::new()),
        ],
    ]);
    let first = test_engine(
        first_model,
        Config {
            auto_approve: true,
            max_turns: 2,
            ..Config::default()
        },
    );
    let error = first
        .run_turn(request("inspect the repository"), &TerminalRenderer::default())
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 2 }));
    assert!(first.session_manager.load_messages().await.unwrap().is_empty());
    let checkpoint = first.session_manager.load_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.len(), 5);
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("repository summary"),
        final_event(Usage::new()),
    ]]);
    let resumed = test_engine_with_session(
        resumed_model.clone(),
        Config {
            auto_approve: true,
            max_turns: 2,
            ..Config::default()
        },
        resumed_store,
    );
    resumed
        .run_turn(request("please continue"), &TerminalRenderer::default())
        .await
        .unwrap();

    let history = &resumed_model.requests()[0].chat_history;
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(encoded.matches("inspect the repository").count(), 1);
    assert_eq!(encoded.matches("missing-a").count(), 2);
    assert_eq!(encoded.matches("missing-b").count(), 2);
    assert_eq!(encoded.matches("please continue").count(), 1);
    assert!(resumed.session_manager.load_checkpoint().await.unwrap().is_none());
    assert_eq!(resumed.session_manager.load_messages().await.unwrap().len(), 7);

    drop(resumed);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_checkpoint().await.unwrap().is_none());
    assert_eq!(reopened.load_messages().await.unwrap().len(), 7);
}

#[tokio::test]
async fn failed_checkpoint_continuation_remains_available_until_success() {
    let first_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path":"missing"})),
        final_event(Usage::new()),
    ]]);
    let first = test_engine(
        first_model,
        Config {
            max_turns: 1,
            ..Config::default()
        },
    );
    first
        .run_turn(request("inspect"), &TerminalRenderer::default())
        .await
        .unwrap_err();
    let checkpoint = first.session_manager.load_checkpoint().await.unwrap().unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("offline provider failure")],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), resumed_store);
    resumed
        .run_turn(request("continue"), &TerminalRenderer::default())
        .await
        .unwrap_err();
    assert_eq!(
        resumed.session_manager.load_checkpoint().await.unwrap(),
        Some(checkpoint)
    );

    resumed
        .run_turn(request("continue again"), &TerminalRenderer::default())
        .await
        .unwrap();
    for request in resumed_model.requests() {
        let history = serde_json::to_string(&request.chat_history).unwrap();
        assert_eq!(history.matches("inspect").count(), 1);
        assert_eq!(history.matches("missing").count(), 2);
    }
    assert!(resumed.session_manager.load_checkpoint().await.unwrap().is_none());

    drop(resumed);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_checkpoint().await.unwrap().is_none());
    assert_eq!(reopened.load_messages().await.unwrap().len(), 5);
}

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
        .run_turn(request("run"), &TerminalRenderer::default())
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
        engine.run_turn(request("run"), &TerminalRenderer::default()),
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
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &renderer,
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call".to_string(),
        tool_name: "read".to_string(),
        arguments: serde_json::json!({"path":"credential-sentinel"}),
        class: ExecutionClass::ReadOnly,
    });
    sink.emit(ToolEvent::Finished {
        internal_call_id: "call".to_string(),
        tool_name: "read".to_string(),
        arguments: serde_json::json!({"path":"credential-sentinel"}),
        output: "credential-sentinel".to_string(),
        status: "error".to_string(),
    });
    let completed = sink.completed();
    assert_eq!(completed.len(), 1);
    assert!(!completed[0].arguments.to_string().contains("credential-sentinel"));
    assert!(!completed[0].output.contains("credential-sentinel"));
    assert!(completed[0].output.contains("[REDACTED]"));
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
