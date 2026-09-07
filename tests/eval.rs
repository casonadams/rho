//! Behavioural evaluation suite for the agent runtime.
//!
//! Each `#[tokio::test]` or `#[test]` here exercises a scenario end-to-end against
//! a real `AgentEngine` backed by `MockCompletionModel`. The local helpers
//! (`DenySink`, `RetryInvalid`, `parts`, `text_occurrences`) are intentionally
//! kept inside this module — they exist only to make the surrounding tests
//! readable.

use rho::engine::eval::context::{ContextComparisonReport, context_comparison};
use rho::engine::eval::harness::{EvalHarness, normalize_requests};
use rho::engine::eval::mock::{MockEngineConfig, final_event, mock_engine, mock_engine_with_session, temp_dir};
use rho::engine::eval::types::{EvalFailure, EvalScenario, NormalizedPart, NormalizedRequest};
use rho::engine::runner::TurnRequest;
use std::path::Path;

use rho_harness_core::error::AppError;
use rho_harness_core::session::{SessionEventKind, SessionManager};

fn builtin_tools_for(dir: &std::path::Path) -> Option<Vec<rig::tool::DynamicTool>> {
    let config = rho::config::Config {
        sessions_dir: dir.join("sessions"),
        ..rho::config::Config::default()
    };
    rho_engine::tools::build_builtin_tools(dir, &config).ok()
}
use rho::presentation::StructuredPresenter;
use rig::agent::AgentBuilder;
use rig::agent::hook::{AgentHook, InvalidToolCallAction, InvalidToolCallContext};
use rig::completion::{CompletionRequest, FinishReason, Usage};
use rig::message::{AssistantContent, Message, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde_json::json;

#[tokio::test]
async fn agent_eval_harness_reports_success() {
    let dir = temp_dir("harness_ok");
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
}

#[tokio::test]
async fn agent_eval_harness_reports_behavior_mismatch() {
    let dir = temp_dir("harness_mismatch");
    std::fs::create_dir_all(&dir).unwrap();
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

fn sample_sensitive_request() -> CompletionRequest {
    CompletionRequest {
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
    }
}

#[test]
fn agent_eval_harness_normalizes_ids_and_omits_content_deterministically() {
    let request = sample_sensitive_request();
    let first = serde_json::to_vec(&normalize_requests(std::slice::from_ref(&request))).unwrap();
    let second = serde_json::to_vec(&normalize_requests(&[request])).unwrap();
    assert_eq!(first, second);
    let encoded = String::from_utf8(first).unwrap();
    assert!(encoded.contains("call-1"));
    for omitted in ["random-id", "credential-sentinel", "secret"] {
        assert!(!encoded.contains(omitted));
    }
}

fn eval_engine(dir: &Path, max_turns: usize, model: MockCompletionModel) -> rho::engine::AgentEngine {
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(dir),
            app_config: rho::config::Config {
                max_turns,
                permission: rho::config::PermissionConfig { enabled: false },
                ..Default::default()
            },
        },
    )
}

fn coding_turns(file: &Path, cmd: &str) -> Vec<Vec<MockStreamEvent>> {
    vec![
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
            MockStreamEvent::tool_call("test-random", "bash", json!({"command": cmd})),
            final_event(Usage::new()),
        ],
        vec![
            MockStreamEvent::text("Changed the value and verification passed."),
            final_event(Usage::new()),
        ],
    ]
}

fn build_coding_scenario(dir: &Path, file: &Path, cmd: String) -> EvalScenario {
    EvalScenario {
        name: "read-edit-test-synthesize",
        prompt: "change and verify",
        turns: coding_turns(file, &cmd),
        expected_final: "Changed the value and verification passed.",
        expected_tools: vec!["read", "edit", "bash"],
        max_turns: 5,
        built_in_tools: builtin_tools_for(dir),
    }
}

#[tokio::test]
async fn agent_eval_core_read_edit_test_and_final_synthesis() {
    let dir = temp_dir("coding");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sample.rs");
    std::fs::write(&file, "fn value() -> u8 { 1 }\n").unwrap();
    let command = format!("grep -q '2' {}", file.display());
    let scenario = build_coding_scenario(&dir, &file, command);

    let report = EvalHarness::run(scenario, &dir).await.unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap(), "fn value() -> u8 { 2 }\n");
    assert_eq!((report.metrics.model_turns, report.metrics.tool_calls), (4, 3));
    assert!(report.transcript.requests.iter().all(|r| !r.content_telemetry));
}

#[tokio::test]
async fn agent_eval_core_multi_tool_order_and_correlation_are_exact() {
    let dir = temp_dir("multi");
    std::fs::create_dir_all(&dir).unwrap();
    let (first, second) = (dir.join("first.txt"), dir.join("second.txt"));
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
    let calls = parts(report.transcript.requests.last().unwrap(), true);
    assert_eq!(calls, [("call-1", "read"), ("call-2", "read")]);
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

struct DenyHook;

impl AgentHook for DenyHook {
    async fn on_tool_call(
        &self,
        _ctx: &rig::agent::hook::HookContext,
        _event: rig::agent::hook::ToolCall<'_>,
    ) -> rig::agent::hook::ToolCallAction {
        rig::agent::hook::ToolCallAction::skip("Operation denied by user; no changes were made.")
    }
}

#[tokio::test]
async fn agent_eval_core_denied_mutation_has_no_side_effect() {
    let dir = temp_dir("denied");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("must-not-exist");
    let model = MockCompletionModel::new([
        rig::test_utils::MockTurn::tool_call("denied-call", "write", json!({"path": marker, "content":"no"})),
        rig::test_utils::MockTurn::text("recovered from denial"),
    ]);
    let agent = AgentBuilder::new(model.clone())
        .tool(rho::tools::WriteTool::new(&dir))
        .add_hook(DenyHook)
        .record_content_telemetry(false)
        .build();
    let response = agent.runner("write").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "recovered from denial");
    assert!(!marker.exists());
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("no changes were made"));
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
async fn agent_eval_core_tool_failure() {
    let dir = temp_dir("recovery_fail");
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
}

#[tokio::test]
async fn agent_eval_core_invalid_tool_recovery() {
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
    assert_eq!((response.output.as_str(), model.request_count()), ("recovered", 2));
}

fn build_repeat_turns(file: &Path) -> Vec<Vec<MockStreamEvent>> {
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
    turns
}

#[tokio::test]
async fn agent_eval_core_repeated_calls_are_steered_on_third_attempt() {
    let dir = temp_dir("repeat");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("input.txt");
    std::fs::write(&file, "same").unwrap();
    let turns = build_repeat_turns(&file);
    let scenario = EvalScenario {
        name: "repeated-baseline",
        prompt: "repeat",
        turns,
        expected_final: "baseline complete",
        expected_tools: vec!["read", "read", "read"],
        max_turns: 5,
        built_in_tools: builtin_tools_for(&dir),
    };
    let report = EvalHarness::run(scenario, &dir).await.unwrap();
    assert_eq!((report.metrics.tool_calls, report.metrics.tool_errors), (2, 1));
}

fn sample_eval_usage() -> Usage {
    Usage {
        input_tokens: 8,
        output_tokens: 3,
        total_tokens: 11,
        cached_input_tokens: 2,
        cache_creation_input_tokens: 1,
        tool_use_prompt_tokens: 0,
        reasoning_tokens: 4,
    }
}

#[tokio::test]
async fn agent_eval_core_finish_metadata_usage() {
    let dir = temp_dir("metadata_usage");
    std::fs::create_dir_all(&dir).unwrap();
    let final_record =
        rig::streaming::StreamFinal::new("mock", sample_eval_usage()).with_finish_reason(FinishReason::Length);
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("partial"),
        MockStreamEvent::FinalResponse(final_record),
    ]]);
    let engine = eval_engine(&dir, 2, model);

    let output = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("generate"),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap();
    assert_eq!(
        output.metrics.completion_calls[0].finish_status.as_deref(),
        Some("length")
    );
    let u = output.metrics.usage.unwrap();
    assert_eq!((u.cached_input_tokens, u.reasoning_tokens), (Some(2), Some(4)));
}

#[tokio::test]
async fn agent_eval_core_budget_exhaustion() {
    let dir = temp_dir("metadata_budget");
    std::fs::create_dir_all(&dir).unwrap();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call", "read", json!({"path": dir.join("none")})),
        final_event(Usage::new()),
    ]]);
    let engine = eval_engine(&dir, 1, model.clone());

    let error = engine
        .run_turn(
            rho::engine::runner::TurnRequest::new("loop"),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    assert_eq!(model.request_count(), 1);
    let events = engine.session_manager.load_events().await.unwrap();
    let summary = events.iter().find(|e| e.kind == SessionEventKind::RunSummary).unwrap();
    assert_eq!(summary.payload["terminal_status"], "budget_exhausted");
}

async fn run_follow_up_turns(engine: &rho::engine::AgentEngine) {
    let presenter = std::sync::Arc::new(StructuredPresenter::stdout());
    engine
        .run_turn(TurnRequest::new("one"), presenter.clone())
        .await
        .unwrap();
    engine.run_turn(TurnRequest::new("two"), presenter).await.unwrap();
}

async fn run_resumed_session(dir: &Path, sid: &str) -> usize {
    let resumed_store = SessionManager::new(&dir.join("sessions"), Some(sid)).unwrap();
    let resumed_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("resumed"), final_event(Usage::new())]]);
    let resumed = mock_engine_with_session(
        resumed_model.clone(),
        MockEngineConfig {
            base_dir: dir,
            session_manager: Some(resumed_store),
            built_in_tools: builtin_tools_for(dir),
            app_config: rho::config::Config {
                max_turns: 3,
                ..Default::default()
            },
        },
    );
    resumed
        .run_turn(
            TurnRequest::new("three"),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap();
    text_occurrences(&resumed_model.requests()[0], "one")
}

#[tokio::test]
async fn agent_eval_session_follow_up_and_resume() {
    let dir = temp_dir("session_resume");
    std::fs::create_dir_all(&dir).unwrap();
    let first_model = MockCompletionModel::from_stream_turns([
        [MockStreamEvent::text("first"), final_event(Usage::new())],
        [MockStreamEvent::text("follow-up"), final_event(Usage::new())],
    ]);
    let first = eval_engine(&dir, 3, first_model.clone());
    run_follow_up_turns(&first).await;
    let occurrences = (
        text_occurrences(&first_model.requests()[1], "one"),
        text_occurrences(&first_model.requests()[1], "first"),
    );
    assert_eq!(occurrences, (1, 1));

    let sid = first.session_manager.session_id.clone();
    drop(first);
    assert_eq!(run_resumed_session(&dir, &sid).await, 1);
}

#[tokio::test]
async fn agent_eval_session_clear_starts_fresh() {
    let dir = temp_dir("session_clear");
    std::fs::create_dir_all(&dir).unwrap();
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
                ..Default::default()
            },
        },
    );
    cleared
        .run_turn(
            TurnRequest::new("fresh prompt"),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap();
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
}

async fn assert_cancelled_session_empty(sm: &SessionManager) {
    let resumed = SessionManager::new(sm.file_path.parent().unwrap(), Some(&sm.session_id)).unwrap();
    assert!(resumed.load_messages().await.unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn agent_eval_session_bash_cancellation_kills_process() {
    let dir = temp_dir("bash-cancel");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("marker");
    let cmd = format!("sleep 2; touch {}", marker.display());
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("bash-call", "bash", json!({"command": cmd})),
        final_event(Usage::new()),
    ]]);
    let engine = eval_engine(&dir, 2, model);
    let timed = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        engine.run_turn(
            TurnRequest::new("run"),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        ),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(timed.is_err());
    engine.record_cancellation("test cancellation").await.unwrap();
    assert!(!marker.exists());
    assert_cancelled_session_empty(&engine.session_manager).await;
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

fn assert_context_usage_reports(reports: &[ContextComparisonReport]) {
    let r0 = &reports[0];
    let r1 = &reports[1];
    assert_eq!((r0.before.input_tokens, r0.after.input_tokens), (Some(120), Some(42)));
    assert!(r0.after.input_tokens < r0.before.input_tokens);
    let counts0 = (
        r0.before.model_visible_messages,
        r0.after.model_visible_messages,
        r0.before.input_tokens,
        r0.after.input_tokens,
    );
    let counts1 = (
        r1.before.model_visible_messages,
        r1.after.model_visible_messages,
        r1.before.input_tokens,
        r1.after.input_tokens,
    );
    assert_eq!(counts0, counts1);
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
    assert_context_usage_reports(&reports);
    let encoded = serde_json::to_string(&reports[0]).unwrap();
    assert!(!encoded.contains("credential-sentinel") && !encoded.contains("historical request"));
}

fn assert_steering_skipped(requests: &[CompletionRequest], second: &Path) {
    assert!(
        !second.exists(),
        "batched write tool call must be skipped after steering"
    );
    assert!(requests.len() >= 2);
    let history = format!("{:?}", requests[1]);
    for p in [
        "[USER STEERING INTERRUPT]",
        "stop modifying files, answer now",
        "Tool execution cancelled due to user steering interrupt.",
    ] {
        assert!(history.contains(p));
    }
}

fn steering_model(first: &Path, second: &Path) -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call-1", "read", json!({"path": first})),
            MockStreamEvent::tool_call(
                "call-2",
                "write",
                json!({"path": second, "content": "should be skipped"}),
            ),
            final_event(Usage::new()),
        ],
        vec![
            MockStreamEvent::text("handled steering gracefully"),
            final_event(Usage::new()),
        ],
    ])
}

#[tokio::test]
async fn agent_eval_mid_turn_steering_skips_subsequent_batched_tools() {
    let dir = temp_dir("steering-eval");
    std::fs::create_dir_all(&dir).unwrap();
    let (first, second) = (dir.join("first.txt"), dir.join("second.txt"));
    std::fs::write(&first, "sample content").unwrap();

    let model = steering_model(&first, &second);
    let engine = eval_engine(&dir, 3, model.clone());
    let steering = std::sync::Arc::new(rho::repl::coordinator::SharedSteeringQueue::new(
        rho::engine::runner::QueueMode::All,
    ));
    steering.enqueue("stop modifying files, answer now".to_string());

    let output = engine
        .run_turn(
            TurnRequest::new("start inspection").with_steering(steering),
            std::sync::Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap();
    assert_eq!(output.final_text, "handled steering gracefully");
    assert_steering_skipped(&model.requests(), &second);
}

fn suppressed_context_config(sessions_dir: std::path::PathBuf) -> rho::config::Config {
    rho::config::Config {
        system_prompt: Some("CLI custom persona".to_string()),
        append_system_prompt: Some("CLI appended rule".to_string()),
        no_context_files: true,
        sessions_dir,
        ..Default::default()
    }
}

#[tokio::test]
async fn agent_eval_context_cli_flags_override_and_suppress() {
    let dir = temp_dir("context-eval");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "# Suppressed Rules\n").unwrap();
    std::fs::write(dir.join("SYSTEM.md"), "Ignored file system prompt\n").unwrap();

    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("done"), final_event(Usage::new())]]);
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(&dir),
            app_config: suppressed_context_config(dir.join("sessions")),
        },
    );

    let ctx = engine.project_context().await.unwrap();
    let prompt = ctx.build_system_prompt();
    assert!(prompt.contains("CLI custom persona") && prompt.contains("CLI appended rule"));
    for excluded in [
        "Ignored file system prompt",
        "<project_instructions",
        "Suppressed Rules",
    ] {
        assert!(!prompt.contains(excluded));
    }
}
