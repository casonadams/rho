use super::AgentEngine;
use super::metrics::{RunMetrics, TerminalStatus};
use super::runner::{TurnOutput, TurnRequest};
use super::runtime::{CodingRuntime, build_coding_agent};
use crate::config::Config;
use crate::session::context::{context_memory, model_visible_bytes};
use crate::session::{SessionEventKind, SessionManager};
use crate::tools::{
    ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalHook, ApprovalRequest, approval_context,
};
use crate::ui::TerminalRenderer;
use async_trait::async_trait;
use rig::agent::hook::{AgentHook, InvalidToolCallAction, InvalidToolCallContext};
use rig::agent::{AgentBuilder, ModelHandle};
use rig::completion::{CompletionRequest, FinishReason, Usage};
use rig::memory::ConversationMemory;
use rig::message::{AssistantContent, Message, UserContent};
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct EvalScenario {
    name: &'static str,
    prompt: &'static str,
    turns: Vec<Vec<MockStreamEvent>>,
    expected_final: &'static str,
    expected_tools: Vec<&'static str>,
    max_turns: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EvalReport {
    scenario: &'static str,
    transcript: NormalizedTranscript,
    metrics: RunMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvalFailure {
    scenario: &'static str,
    behavior: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedTranscript {
    requests: Vec<NormalizedRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedRequest {
    index: usize,
    content_telemetry: bool,
    messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedMessage {
    role: &'static str,
    parts: Vec<NormalizedPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NormalizedPart {
    Text,
    Reasoning,
    Image,
    Audio,
    Video,
    Document,
    ToolCall { id: String, name: String },
    ToolResult { id: String, name: String },
}

struct EvalHarness;

impl EvalHarness {
    async fn run(scenario: EvalScenario, base_dir: &Path) -> Result<EvalReport, EvalFailure> {
        let model = MockCompletionModel::from_stream_turns(scenario.turns.clone());
        let engine = mock_engine(model.clone(), base_dir, scenario.max_turns);
        let output = engine
            .run_turn(
                TurnRequest {
                    prompt: scenario.prompt,
                    intent: None,
                },
                &TerminalRenderer::default(),
            )
            .await
            .map_err(|_| EvalFailure {
                scenario: scenario.name,
                behavior: "scenario execution failed",
            })?;
        verify_output(&scenario, &output)?;
        let transcript = normalize_requests(&model.requests());
        verify_tool_order(&scenario, &transcript)?;
        Ok(EvalReport {
            scenario: scenario.name,
            transcript,
            metrics: output.metrics.normalized(),
        })
    }
}

fn verify_output(scenario: &EvalScenario, output: &TurnOutput) -> Result<(), EvalFailure> {
    if output.final_text != scenario.expected_final {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "final answer mismatch",
        });
    }
    if output.metrics.terminal_status != TerminalStatus::Completed {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "terminal status mismatch",
        });
    }
    Ok(())
}

fn verify_tool_order(scenario: &EvalScenario, transcript: &NormalizedTranscript) -> Result<(), EvalFailure> {
    let names = transcript
        .requests
        .last()
        .map(|request| {
            request
                .messages
                .iter()
                .flat_map(|message| &message.parts)
                .filter_map(|part| match part {
                    NormalizedPart::ToolCall { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if names != scenario.expected_tools {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "tool ordering mismatch",
        });
    }
    Ok(())
}

fn normalize_requests(requests: &[CompletionRequest]) -> NormalizedTranscript {
    let mut ids = HashMap::new();
    let requests = requests
        .iter()
        .enumerate()
        .map(|(index, request)| NormalizedRequest {
            index,
            content_telemetry: request.record_telemetry_content,
            messages: request
                .chat_history
                .iter()
                .map(|message| normalize_message(message, &mut ids))
                .collect(),
        })
        .collect();
    NormalizedTranscript { requests }
}

fn normalize_message(message: &Message, ids: &mut HashMap<String, String>) -> NormalizedMessage {
    match message {
        Message::System { .. } => NormalizedMessage {
            role: "system",
            parts: vec![NormalizedPart::Text],
        },
        Message::User { content } => NormalizedMessage {
            role: "user",
            parts: content.iter().map(|part| normalize_user_part(part, ids)).collect(),
        },
        Message::Assistant { content, .. } => NormalizedMessage {
            role: "assistant",
            parts: content.iter().map(|part| normalize_assistant_part(part, ids)).collect(),
        },
    }
}

fn normalize_user_part(part: &UserContent, ids: &mut HashMap<String, String>) -> NormalizedPart {
    match part {
        UserContent::Text(_) => NormalizedPart::Text,
        UserContent::ToolResult(result) => NormalizedPart::ToolResult {
            id: normalize_id(result.call.as_str(), ids),
            name: result.name.clone(),
        },
        UserContent::Image(_) => NormalizedPart::Image,
        UserContent::Audio(_) => NormalizedPart::Audio,
        UserContent::Video(_) => NormalizedPart::Video,
        UserContent::Document(_) => NormalizedPart::Document,
    }
}

fn normalize_assistant_part(part: &AssistantContent, ids: &mut HashMap<String, String>) -> NormalizedPart {
    match part {
        AssistantContent::Text(_) => NormalizedPart::Text,
        AssistantContent::ToolCall(call) => NormalizedPart::ToolCall {
            id: normalize_id(call.id.as_str(), ids),
            name: call.function.name.clone(),
        },
        AssistantContent::Reasoning(_) => NormalizedPart::Reasoning,
        AssistantContent::Image(_) => NormalizedPart::Image,
    }
}

fn normalize_id(id: &str, ids: &mut HashMap<String, String>) -> String {
    let next = format!("call-{}", ids.len() + 1);
    ids.entry(id.to_string()).or_insert(next).clone()
}

struct MockEngineConfig<'a> {
    base_dir: &'a Path,
    max_turns: usize,
    session_manager: SessionManager,
}

fn mock_engine(model: MockCompletionModel, base_dir: &Path, max_turns: usize) -> AgentEngine {
    let sessions = base_dir.join("sessions");
    let session_manager = SessionManager::new(&sessions, None).unwrap();
    mock_engine_with_session(
        model,
        MockEngineConfig {
            base_dir,
            max_turns,
            session_manager,
        },
    )
}

fn mock_engine_with_session(model: MockCompletionModel, config: MockEngineConfig<'_>) -> AgentEngine {
    let app_config = Config {
        auto_approve: true,
        max_turns: config.max_turns,
        sessions_dir: config.base_dir.join("sessions"),
        ..Config::default()
    };
    let agent = build_coding_agent(
        ModelHandle::new(model),
        &app_config,
        CodingRuntime {
            base_dir: config.base_dir,
            memory: config.session_manager.clone(),
        },
    )
    .unwrap();
    AgentEngine {
        config: app_config,
        session_manager: config.session_manager,
        agent,
        last_usage: Mutex::new(None),
        run_tracker: super::metrics::RunTracker::default(),
    }
}

fn final_event(usage: Usage) -> MockStreamEvent {
    MockStreamEvent::final_response(usage)
}

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agent_eval_{label}_{}", uuid::Uuid::new_v4()))
}

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
        .tool(crate::tools::WriteTool::new(&dir))
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
    let engine = mock_engine(model, &dir, 2);
    let output = engine
        .run_turn(
            TurnRequest {
                prompt: "generate",
                intent: None,
            },
            &TerminalRenderer::default(),
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
    let engine = mock_engine(model.clone(), &dir, 1);
    let error = engine
        .run_turn(
            TurnRequest {
                prompt: "loop",
                intent: None,
            },
            &TerminalRenderer::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        crate::error::AppError::ModelBudgetExhausted { max_turns: 1 }
    ));
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
    let first = mock_engine(first_model.clone(), &dir, 3);
    first
        .run_turn(
            TurnRequest {
                prompt: "one",
                intent: None,
            },
            &TerminalRenderer::default(),
        )
        .await
        .unwrap();
    first
        .run_turn(
            TurnRequest {
                prompt: "two",
                intent: None,
            },
            &TerminalRenderer::default(),
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
            max_turns: 3,
            session_manager: resumed_store,
        },
    );
    resumed
        .run_turn(
            TurnRequest {
                prompt: "three",
                intent: None,
            },
            &TerminalRenderer::default(),
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
            max_turns: 3,
            session_manager: resumed.session_manager.clone(),
        },
    );
    switched
        .run_turn(
            TurnRequest {
                prompt: "four",
                intent: None,
            },
            &TerminalRenderer::default(),
        )
        .await
        .unwrap();
    assert_eq!(text_occurrences(&switched_model.requests()[0], "three"), 1);

    let cleared_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("fresh"), final_event(Usage::new())]]);
    let cleared = mock_engine(cleared_model.clone(), &dir.join("clear"), 2);
    cleared
        .run_turn(
            TurnRequest {
                prompt: "fresh prompt",
                intent: None,
            },
            &TerminalRenderer::default(),
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
        let engine = mock_engine(model, &dir, 2);
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(40),
            engine.run_turn(
                TurnRequest {
                    prompt: "run",
                    intent: None,
                },
                &TerminalRenderer::default(),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ContextComparisonReport {
    scenario: &'static str,
    before: ContextEvaluation,
    after: ContextEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ContextEvaluation {
    model_visible_messages: usize,
    model_visible_bytes: usize,
    input_tokens: Option<u64>,
    success: bool,
    terminal_status: TerminalStatus,
    turns: usize,
    tool_calls: usize,
    tool_errors: usize,
    tool_denials: usize,
    usage_available: bool,
}

struct ContextEvaluationInput<'a> {
    base_dir: &'a Path,
    history: &'a [Message],
    bounded: bool,
    usage: Usage,
}

async fn run_context_evaluation(input: ContextEvaluationInput<'_>) -> ContextEvaluation {
    let ContextEvaluationInput {
        base_dir,
        history,
        bounded,
        usage,
    } = input;
    let sessions = base_dir.join(if bounded { "bounded" } else { "full" });
    let store = SessionManager::new(&sessions, None).unwrap();
    let id = store.session_id.clone();
    ConversationMemory::append(&store, &id, history.to_vec()).await.unwrap();
    let memory: Arc<dyn ConversationMemory> = if bounded {
        context_memory(store.clone(), 4, 512)
    } else {
        Arc::new(store.clone())
    };
    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("completed"), final_event(usage)]]);
    let config = Config {
        auto_approve: true,
        max_turns: 2,
        sessions_dir: sessions,
        ..Config::default()
    };
    let agent = AgentBuilder::from_model_handle(ModelHandle::new(model.clone()))
        .memory(memory)
        .record_content_telemetry(false)
        .build();
    let engine = AgentEngine {
        config,
        session_manager: store,
        agent,
        last_usage: Mutex::new(None),
        run_tracker: super::metrics::RunTracker::default(),
    };
    let output = engine
        .run_turn(
            TurnRequest {
                prompt: "continue",
                intent: None,
            },
            &TerminalRenderer::default(),
        )
        .await
        .unwrap();
    let visible = &model.requests()[0].chat_history;
    ContextEvaluation {
        model_visible_messages: visible.len(),
        model_visible_bytes: model_visible_bytes(visible),
        input_tokens: output.usage.map(|usage| usage.input_tokens),
        success: output.metrics.success,
        terminal_status: output.metrics.terminal_status,
        turns: output.metrics.model_turns,
        tool_calls: output.metrics.tool_calls,
        tool_errors: output.metrics.tool_errors,
        tool_denials: output.metrics.tool_denials,
        usage_available: output.metrics.usage_available,
    }
}

fn long_context_history() -> Vec<Message> {
    (0..30)
        .flat_map(|index| {
            [
                Message::user(format!("historical request {index}: {}", "context ".repeat(30))),
                Message::assistant(format!("historical response {index}: {}", "result ".repeat(30))),
            ]
        })
        .collect()
}

async fn context_comparison(base_dir: &Path, before_usage: Usage, after_usage: Usage) -> ContextComparisonReport {
    let history = long_context_history();
    ContextComparisonReport {
        scenario: "long-session-context",
        before: run_context_evaluation(ContextEvaluationInput {
            base_dir,
            history: &history,
            bounded: false,
            usage: before_usage,
        })
        .await,
        after: run_context_evaluation(ContextEvaluationInput {
            base_dir,
            history: &history,
            bounded: true,
            usage: after_usage,
        })
        .await,
    }
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
    let first = serde_json::to_vec(&reports[0]).unwrap();
    let second = serde_json::to_vec(&reports[1]).unwrap();
    assert_eq!(first, second);
    let encoded = String::from_utf8(first).unwrap();
    assert!(!encoded.contains("credential-sentinel"));
    assert!(!encoded.contains("historical request"));
}
