//! Behavioural evaluation suite for the agent runtime.
//!
//! Each `#[tokio::test]` or `#[test]` here exercises a scenario end-to-end against
//! a real `AgentEngine` backed by `MockCompletionModel`. The local helpers
//! (`DenySink`, `RetryInvalid`, `parts`, `text_occurrences`) are intentionally
//! kept inside this module — they exist only to make the surrounding tests
//! readable.

use rho::engine::eval::context::context_comparison;
use rho::engine::eval::harness::{EvalHarness, normalize_requests};
use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine, mock_engine_with_session, temp_dir};
use rho::engine::eval::types::{EvalFailure, EvalScenario, NormalizedPart, NormalizedRequest};
use rho::engine::runner::TurnRequest;

use rho::approval::hook::ApprovalHook;
use rho::plugin::tool_dispatch::ActiveToolSet;
use rho_core::error::AppError;
use rho_core::session::{SessionEventKind, SessionManager};

fn builtin_tools_for(dir: &std::path::Path) -> Option<Vec<rig::tool::DynamicTool>> {
    let config = rho::config::Config {
        sessions_dir: dir.join("sessions"),
        ..rho::config::Config::default()
    };
    Some(
        ActiveToolSet::builtins(&config, dir)
            .expect("builtin platform tools")
            .into_rig_tools(),
    )
}
use async_trait::async_trait;
use rho::tools::{ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, approval_context};
use rho::ui::TerminalRenderer;
use rig::agent::AgentBuilder;
use rig::agent::hook::{AgentHook, InvalidToolCallAction, InvalidToolCallContext};
use rig::completion::{CompletionRequest, FinishReason, Usage};
use rig::message::{AssistantContent, Message, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn agent_eval_harness_reports_success_and_behavior_mismatch() {
    let dir = temp_dir("harness");
    std::fs::create_dir_all(&dir).unwrap();
    let passing = EvalScenario {
        name: "final-synthesis",
        prompt: "summarize",
        turns: vec![vec![MockStreamEvent::text("summary"), final_event(Usage::new())]],
        expected_final: "summary",
        expected_tools: vec![],
        max_turns: 2,
        built_in_tools: builtin_tools_for(&dir),
    };
    assert_eq!(
        EvalHarness::run(passing, &dir).await.unwrap().scenario,
        "final-synthesis"
    );

    let mismatch = EvalScenario {
        name: "mismatch",
        prompt: "summarize",
        turns: vec![vec![MockStreamEvent::text("actual"), final_event(Usage::new())]],
        expected_final: "different",
        expected_tools: vec![],
        max_turns: 2,
        built_in_tools: builtin_tools_for(&dir),
    };
    let error = EvalHarness::run(mismatch, &dir).await.unwrap_err();
    assert_eq!(error.behavior, "final answer mismatch");
}

#[tokio::test]
async fn agent_eval_harness_rejects_malformed_scripted_events_without_leaking_them() {
    let dir = temp_dir("malformed");
    std::fs::create_dir_all(&dir).unwrap();
    let scenario = EvalScenario {
        name: "malformed-event",
        prompt: "run",
        turns: vec![vec![MockStreamEvent::text_additional_params(json!({}))]],
        expected_final: "",
        expected_tools: vec![],
        max_turns: 2,
        built_in_tools: builtin_tools_for(&dir),
    };
    let error = EvalHarness::run(scenario, &dir).await.unwrap_err();
    assert_eq!(error.behavior, "scenario execution failed");
}

#[test]
fn agent_eval_harness_normalizes_ids_and_omits_content_deterministically() {
    let request = CompletionRequest {
        model: None,
        preamble: None,
        chat_history: vec![
            Message::user("credential-sentinel"),
            Message::Assistant {
                id: Some("provider-secret".to_string()),
                content: vec![AssistantContent::tool_call(
                    "random-id",
                    "read",
                    json!({"path":"secret"}),
                )],
            },
        ],
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    };
    let first = serde_json::to_vec(&normalize_requests(std::slice::from_ref(&request))).unwrap();
    let second = serde_json::to_vec(&normalize_requests(&[request])).unwrap();
    assert_eq!(first, second);
    let encoded = String::from_utf8(first).unwrap();
    assert!(encoded.contains("call-1"));
    assert!(!encoded.contains("random-id"));
    assert!(!encoded.contains("credential-sentinel"));
    assert!(!encoded.contains("secret"));
}

#[tokio::test]
async fn agent_eval_core_read_edit_test_and_final_synthesis() {
    let dir = temp_dir("coding");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sample.rs");
    std::fs::write(&file, "fn value() -> u8 { 1 }\n").unwrap();
    let command = format!("grep -q '2' {}", file.display());
    let scenario = EvalScenario {
        name: "read-edit-test-synthesize",
        prompt: "change and verify",
        turns: vec![
            vec![
                MockStreamEvent::tool_call("read-random", "read", json!({"path": file})),
                final_event(Usage::new()),
            ],
            vec![
                MockStreamEvent::tool_call(
                    "edit-random",
                    "edit",
                    json!({"path": file, "edits":[{"oldText":"{ 1 }", "newText":"{ 2 }"}]}),
                ),
                final_event(Usage::new()),
            ],
            vec![
                MockStreamEvent::tool_call("test-random", "bash", json!({"command": command})),
                final_event(Usage::new()),
            ],
            vec![
                MockStreamEvent::text("Changed the value and verification passed."),
                final_event(Usage::new()),
            ],
        ],
        expected_final: "Changed the value and verification passed.",
        expected_tools: vec!["read", "edit", "bash"],
        max_turns: 5,
        built_in_tools: builtin_tools_for(&dir),
    };
    let report = EvalHarness::run(scenario, &dir).await.unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap(), "fn value() -> u8 { 2 }\n");
    assert_eq!((report.metrics.model_turns, report.metrics.tool_calls), (4, 3));
    assert!(
        report
            .transcript
            .requests
            .iter()
            .all(|request| !request.content_telemetry)
    );
}

#[tokio::test]
async fn agent_eval_core_multi_tool_order_and_correlation_are_exact() {
    let dir = temp_dir("multi");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    std::fs::write(&first, "one").unwrap();
    std::fs::write(&second, "two").unwrap();
    let scenario = EvalScenario {
        name: "multi-tool",
        prompt: "inspect both",
        turns: vec![
            vec![
                MockStreamEvent::tool_call("wire-b", "read", json!({"path": first})),
                MockStreamEvent::tool_call("wire-a", "read", json!({"path": second})),
                final_event(Usage::new()),
            ],
            vec![MockStreamEvent::text("both inspected"), final_event(Usage::new())],
        ],
        expected_final: "both inspected",
        expected_tools: vec!["read", "read"],
        max_turns: 3,
        built_in_tools: builtin_tools_for(&dir),
    };
    let report = EvalHarness::run(scenario, &dir).await.unwrap();
    let final_request = report.transcript.requests.last().unwrap();
    let calls = parts(final_request, true);
    let results = parts(final_request, false);
    assert_eq!(calls, [("call-1", "read"), ("call-2", "read")]);
    assert_eq!(results, calls);
}

fn parts(request: &NormalizedRequest, calls: bool) -> Vec<(&str, &str)> {
    request
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match (calls, part) {
            (true, NormalizedPart::ToolCall { id, name }) | (false, NormalizedPart::ToolResult { id, name }) => {
                Some((id.as_str(), name.as_str()))
            }
            _ => None,
        })
        .collect()
}

struct DenySink;

#[async_trait]
impl ApprovalEventSink for DenySink {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Denied {
            reason: "baseline denial".to_string(),
        }
    }
}

#[tokio::test]
async fn agent_eval_core_denied_mutation_has_no_side_effect() {
    let dir = temp_dir("denied");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("must-not-exist");
    let capability = ApprovalCapability::new(false, Arc::new(DenySink));
    let model = MockCompletionModel::new([
        rig::test_utils::MockTurn::tool_call("denied-call", "write", json!({"path": marker, "content":"no"})),
        rig::test_utils::MockTurn::text("recovered from denial"),
    ]);
    let agent = AgentBuilder::new(model.clone())
        .tool(rho::tools::WriteTool::new(&dir))
        .add_hook(ApprovalHook::new(capability.clone()))
        .record_content_telemetry(false)
        .build();
    let response = agent
        .runner("write")
        .tool_context(approval_context(capability))
        .tool_concurrency(1)
        .max_turns(3)
        .run()
        .await
        .unwrap();
    assert_eq!(response.output, "recovered from denial");
    assert!(!marker.exists());
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("No changes were made"));
}

struct RetryInvalid;

impl AgentHook for RetryInvalid {
    async fn on_invalid_tool_call(
        &self,
        _ctx: &rig::agent::hook::HookContext,
        _event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        Some(InvalidToolCallAction::retry("choose an available tool"))
    }
}

#[tokio::test]
async fn agent_eval_core_tool_failure_and_invalid_tool_recovery() {
    let dir = temp_dir("recovery");
    std::fs::create_dir_all(&dir).unwrap();
    let failure = EvalScenario {
        name: "tool-failure",
        prompt: "read missing",
        turns: vec![
            vec![
                MockStreamEvent::tool_call("missing", "read", json!({"path": dir.join("missing")})),
                final_event(Usage::new()),
            ],
            vec![
                MockStreamEvent::text("reported missing file"),
                final_event(Usage::new()),
            ],
        ],
        expected_final: "reported missing file",
        expected_tools: vec!["read"],
        max_turns: 3,
        built_in_tools: builtin_tools_for(&dir),
    };
    let report = EvalHarness::run(failure, &dir).await.unwrap();
    assert_eq!(report.metrics.tool_errors, 1);

    let model = MockCompletionModel::new([
        rig::test_utils::MockTurn::tool_call("bad", "not_registered", json!({})),
        rig::test_utils::MockTurn::text("recovered"),
    ]);
    let agent = AgentBuilder::new(model.clone()).add_hook(RetryInvalid).build();
    let response = agent
        .runner("recover")
        .max_invalid_tool_call_retries(1)
        .max_turns(2)
        .run()
        .await
        .unwrap();
    assert_eq!(response.output, "recovered");
    assert_eq!(model.request_count(), 2);
}

#[tokio::test]
async fn agent_eval_core_repeated_calls_are_steered_on_third_attempt() {
    let dir = temp_dir("repeat");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("input.txt");
    std::fs::write(&file, "same").unwrap();
    let mut turns = Vec::new();
    for id in ["repeat-a", "repeat-b", "repeat-c"] {
        turns.push(vec![
            MockStreamEvent::tool_call(id, "read", json!({"path": file})),
            final_event(Usage::new()),
        ]);
    }
    turns.push(vec![
        MockStreamEvent::text("baseline complete"),
        final_event(Usage::new()),
    ]);
    let report = EvalHarness::run(
        EvalScenario {
            name: "repeated-baseline",
            prompt: "repeat",
            turns,
            expected_final: "baseline complete",
            expected_tools: vec!["read", "read", "read"],
            max_turns: 5,
            built_in_tools: builtin_tools_for(&dir),
        },
        &dir,
    )
    .await
    .unwrap();
    assert_eq!(report.metrics.tool_calls, 3);
    assert_eq!(report.metrics.tool_errors, 1);
}

#[tokio::test]
async fn agent_eval_core_finish_metadata_usage_and_budget_exhaustion() {
    let dir = temp_dir("metadata");
    std::fs::create_dir_all(&dir).unwrap();
    let usage = Usage {
        input_tokens: 8,
        output_tokens: 3,
        total_tokens: 11,
        cached_input_tokens: 2,
        cache_creation_input_tokens: 1,
        tool_use_prompt_tokens: 0,
        reasoning_tokens: 4,
    };
    let final_record = rig::streaming::StreamFinal::new("mock", usage).with_finish_reason(FinishReason::Length);
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("partial"),
        MockStreamEvent::FinalResponse(final_record),
    ]]);
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(&dir),
            app_config: rho::config::Config {
                max_turns: 2,
                ..rho::config::Config::default()
            },
        },
    );
    let output = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("generate"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    assert_eq!(
        output.metrics.completion_calls[0].finish_status.as_deref(),
        Some("length")
    );
    assert_eq!(output.metrics.usage.unwrap().cached_input_tokens, Some(2));
    assert_eq!(output.metrics.usage.unwrap().reasoning_tokens, Some(4));

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call", "read", json!({"path": dir.join("none")})),
        final_event(Usage::new()),
    ]]);
    let engine = mock_engine(
        model.clone(),
        MockEngineConfig {
            base_dir: &dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(&dir),
            app_config: rho::config::Config {
                max_turns: 1,
                ..rho::config::Config::default()
            },
        },
    );
    let error = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("loop"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    assert_eq!(model.request_count(), 1);
    let events = engine.session_manager.load_events().await.unwrap();
    let summary = events
        .iter()
        .find(|event| event.kind == SessionEventKind::RunSummary)
        .unwrap();
    assert_eq!(summary.payload["terminal_status"], "budget_exhausted");
}

#[tokio::test]
async fn agent_eval_session_follow_up_resume_clear_and_model_switch() {
    let dir = temp_dir("session");
    std::fs::create_dir_all(&dir).unwrap();
    let first_model = MockCompletionModel::from_stream_turns([
        [MockStreamEvent::text("first"), final_event(Usage::new())],
        [MockStreamEvent::text("follow-up"), final_event(Usage::new())],
    ]);
    let first = mock_engine(
        first_model.clone(),
        MockEngineConfig {
            base_dir: &dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(&dir),
            app_config: rho::config::Config {
                max_turns: 3,
                ..rho::config::Config::default()
            },
        },
    );
    first
        .run_turn(
            TurnRequest::new("one"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    first
        .run_turn(
            TurnRequest::new("two"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    assert_eq!(text_occurrences(&first_model.requests()[1], "one"), 1);
    assert_eq!(text_occurrences(&first_model.requests()[1], "first"), 1);

    let session_id = first.session_manager.session_id.clone();
    let prior_file = first.session_manager.file_path.clone();
    drop(first);
    let resumed_store = SessionManager::new(&dir.join("sessions"), Some(&session_id)).unwrap();
    let resumed_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("resumed"), final_event(Usage::new())]]);
    let resumed = mock_engine_with_session(
        resumed_model.clone(),
        MockEngineConfig {
            base_dir: &dir,
            session_manager: Some(resumed_store),
            built_in_tools: builtin_tools_for(&dir),
            app_config: rho::config::Config {
                max_turns: 3,
                ..rho::config::Config::default()
            },
        },
    );
    resumed
        .run_turn(
            TurnRequest::new("three"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    assert_eq!(text_occurrences(&resumed_model.requests()[0], "one"), 1);

    let switched_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("switched"), final_event(Usage::new())]]);
    let switched = mock_engine_with_session(
        switched_model.clone(),
        MockEngineConfig {
            base_dir: &dir,
            built_in_tools: builtin_tools_for(&dir),
            session_manager: Some(resumed.session_manager.clone()),
            app_config: rho::config::Config {
                max_turns: 3,
                ..rho::config::Config::default()
            },
        },
    );
    switched
        .run_turn(
            TurnRequest::new("four"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    assert_eq!(text_occurrences(&switched_model.requests()[0], "three"), 1);

    let cleared_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("fresh"), final_event(Usage::new())]]);
    let cleared = mock_engine(
        cleared_model.clone(),
        MockEngineConfig {
            base_dir: &dir.join("clear"),
            session_manager: None,
            built_in_tools: builtin_tools_for(&dir.join("clear")),
            app_config: rho::config::Config {
                max_turns: 2,
                ..rho::config::Config::default()
            },
        },
    );
    cleared
        .run_turn(
            TurnRequest::new("fresh prompt"),
            std::sync::Arc::new(TerminalRenderer::default()),
        )
        .await
        .unwrap();
    assert!(prior_file.exists());
    assert_eq!(cleared_model.requests()[0].chat_history.len(), 2);
}

fn text_occurrences(request: &CompletionRequest, needle: &str) -> usize {
    request
        .chat_history
        .iter()
        .map(|message| match message {
            Message::User { content } => content
                .iter()
                .filter(|part| matches!(part, UserContent::Text(text) if text.text == needle))
                .count(),
            Message::Assistant { content, .. } => content
                .iter()
                .filter(|part| matches!(part, AssistantContent::Text(text) if text.text == needle))
                .count(),
            Message::System { .. } => 0,
        })
        .sum()
}

#[tokio::test]
async fn agent_eval_session_cancellation_boundaries_remain_resumable() {
    for boundary in ["before_output", "during_output", "around_tool"] {
        let dir = temp_dir(boundary);
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        store
            .append_event(
                SessionEventKind::Cancellation,
                json!({"boundary": boundary, "terminal": true}),
            )
            .await
            .unwrap();
        drop(store);
        let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(resumed.load_messages().await.unwrap().is_empty());
    }

    #[cfg(unix)]
    {
        let dir = temp_dir("bash-cancel");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("marker");
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::tool_call(
                "bash-call",
                "bash",
                json!({"command": format!("sleep 2; touch {}", marker.display())}),
            ),
            final_event(Usage::new()),
        ]]);
        let engine = mock_engine(
            model,
            MockEngineConfig {
                base_dir: &dir,
                session_manager: None,
                built_in_tools: builtin_tools_for(&dir),
                app_config: rho::config::Config {
                    auto_approve: true,
                    max_turns: 2,
                    ..rho::config::Config::default()
                },
            },
        );
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(40),
            engine.run_turn(
                TurnRequest::new("run"),
                std::sync::Arc::new(TerminalRenderer::default()),
            ),
        )
        .await;
        assert!(timed.is_err());
        engine.record_cancellation("test cancellation").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert!(!marker.exists());
        let resumed = SessionManager::new(
            engine.session_manager.file_path.parent().unwrap(),
            Some(&engine.session_manager.session_id),
        )
        .unwrap();
        assert!(resumed.load_messages().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn agent_eval_reports_are_stable_and_secret_free() {
    let mut reports = Vec::new();
    for label in ["stable-a", "stable-b"] {
        let dir = temp_dir(label);
        std::fs::create_dir_all(&dir).unwrap();
        reports.push(
            EvalHarness::run(
                EvalScenario {
                    name: "stable",
                    prompt: "stable prompt",
                    turns: vec![vec![MockStreamEvent::text("stable answer"), final_event(Usage::new())]],
                    expected_final: "stable answer",
                    expected_tools: vec![],
                    max_turns: 2,
                    built_in_tools: builtin_tools_for(&dir),
                },
                &dir,
            )
            .await
            .unwrap(),
        );
    }
    let first = serde_json::to_vec(&reports[0]).unwrap();
    let second = serde_json::to_vec(&reports[1]).unwrap();
    assert_eq!(first, second);
    assert!(!String::from_utf8(first).unwrap().contains("credential-sentinel"));
}

#[test]
fn evaluation_errors_do_not_include_expected_or_observed_content() {
    let error = EvalFailure {
        scenario: "redaction",
        behavior: "final answer mismatch",
    };
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("credential-sentinel"));
}

#[tokio::test]
async fn agent_eval_context_reduces_visible_history_without_success_regression() {
    let dir = temp_dir("context-comparison");
    std::fs::create_dir_all(&dir).unwrap();
    let report = context_comparison(&dir, Usage::new(), Usage::new()).await;

    assert!(report.before.success && report.after.success);
    assert_eq!(report.before.terminal_status, report.after.terminal_status);
    assert!(report.after.model_visible_messages < report.before.model_visible_messages);
    assert!(report.after.model_visible_bytes < report.before.model_visible_bytes);
    assert_eq!(report.before.input_tokens, None);
    assert_eq!(report.after.input_tokens, None);
    assert!(!report.before.usage_available && !report.after.usage_available);
}

#[tokio::test]
async fn agent_eval_context_reports_usage_only_when_available_and_is_deterministic() {
    let before_usage = Usage {
        input_tokens: 120,
        output_tokens: 3,
        total_tokens: 123,
        ..Usage::new()
    };
    let after_usage = Usage {
        input_tokens: 42,
        output_tokens: 3,
        total_tokens: 45,
        ..Usage::new()
    };
    let mut reports = Vec::new();
    for label in ["context-stable-a", "context-stable-b"] {
        let dir = temp_dir(label);
        std::fs::create_dir_all(&dir).unwrap();
        reports.push(context_comparison(&dir, before_usage, after_usage).await);
    }

    assert_eq!(reports[0].before.input_tokens, Some(120));
    assert_eq!(reports[0].after.input_tokens, Some(42));
    assert!(reports[0].after.input_tokens < reports[0].before.input_tokens);
    assert_eq!(
        reports[0].before.model_visible_messages,
        reports[1].before.model_visible_messages
    );
    assert_eq!(
        reports[0].after.model_visible_messages,
        reports[1].after.model_visible_messages
    );
    assert_eq!(reports[0].before.input_tokens, reports[1].before.input_tokens);
    assert_eq!(reports[0].after.input_tokens, reports[1].after.input_tokens);
    let encoded = serde_json::to_string(&reports[0]).unwrap();
    assert!(!encoded.contains("credential-sentinel"));
    assert!(!encoded.contains("historical request"));
}
