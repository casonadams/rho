//! Behavioural evaluation suite for the agent runtime.

use rho::engine::eval::harness::{EvalHarness, normalize_requests};
use rho::engine::eval::mock::{MockEngineConfig, mock_engine, mock_engine_with_session, temp_dir};
use rho::engine::eval::types::{EvalFailure, EvalScenario};
use rho::engine::runner::TurnRequest;
use std::path::Path;
use std::sync::Arc;

use rho_engine::provider::adapter::{FinishReason, MockAdapter, ModelStreamEvent};
use rho_engine::tools::EngineTool;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, ToolCall, ToolFunction};
use rho_harness_core::session::{SessionEventKind, SessionManager};
use serde_json::json;

fn builtin_tools_for(dir: &std::path::Path) -> Option<Vec<Arc<dyn EngineTool>>> {
    let config = rho::config::Config {
        sessions_dir: dir.join("sessions"),
        ..rho::config::Config::default()
    };
    rho_engine::tools::build_all_builtin_tools(dir, &config).ok()
}

use rho::presentation::StructuredPresenter;

#[tokio::test]
async fn agent_eval_harness_reports_success() {
    let dir = temp_dir("harness_ok");
    std::fs::create_dir_all(&dir).unwrap();
    let passing = EvalScenario {
        name: "final-synthesis",
        prompt: "summarize",
        turns: vec![vec![ModelStreamEvent::Text("summary".to_string())]],
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
        turns: vec![vec![ModelStreamEvent::Text("actual".to_string())]],
        expected_final: "different",
        expected_tools: vec![],
        max_turns: 2,
        built_in_tools: builtin_tools_for(&dir),
    };
    let error = EvalHarness::run(mismatch, &dir).await.unwrap_err();
    assert_eq!(error.behavior, "final answer mismatch");
}

fn sample_sensitive_request() -> rho_engine::provider::adapter::ModelCompletionRequest {
    rho_engine::provider::adapter::ModelCompletionRequest {
        preamble: None,
        prompt: "credential-sentinel".to_string(),
        messages: vec![ChatMessage::assistant_blocks(vec![
            rho_harness_core::model::AssistantContent::ToolCall(ToolCall::new(
                "random-id",
                ToolFunction::new("read", json!({"path":"secret"})),
            )),
        ])],
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
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

fn eval_engine(dir: &Path, max_turns: usize, model: Arc<MockAdapter>) -> rho::engine::AgentEngine {
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            session_manager: None,
            built_in_tools: builtin_tools_for(dir),
            app_config: rho::config::Config {
                max_turns,
                ..Default::default()
            },
        },
    )
}

#[tokio::test]
async fn agent_eval_core_tool_failure() {
    let dir = temp_dir("recovery_fail");
    std::fs::create_dir_all(&dir).unwrap();
    let failure = EvalScenario {
        name: "tool-failure",
        prompt: "read missing",
        turns: vec![
            vec![ModelStreamEvent::ToolCall(ToolCall::new(
                "missing",
                ToolFunction::new("read", json!({"path": dir.join("missing")})),
            ))],
            vec![ModelStreamEvent::Text("reported missing file".to_string())],
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
async fn agent_eval_core_finish_metadata_usage() {
    let dir = temp_dir("metadata_usage");
    std::fs::create_dir_all(&dir).unwrap();
    let usage = rho_engine::engine::metrics::StructuralUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        cached_input_tokens: Some(2),
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: Some(4),
    };
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![
        Ok(ModelStreamEvent::Text("partial".to_string())),
        Ok(ModelStreamEvent::Usage(usage)),
        Ok(ModelStreamEvent::Finish(FinishReason::Length)),
    ]);
    let engine = eval_engine(&dir, 2, model);

    let output = engine
        .run_turn(TurnRequest::new("generate"), Arc::new(StructuredPresenter::stdout()))
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
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call",
        ToolFunction::new("read", json!({"path": dir.join("none")})),
    )))]);
    let engine = eval_engine(&dir, 1, model.clone());

    let error = engine
        .run_turn(TurnRequest::new("loop"), Arc::new(StructuredPresenter::stdout()))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 1 }));
    let events = engine.session_manager.load_events().await.unwrap();
    let summary = events.iter().find(|e| e.kind == SessionEventKind::RunSummary).unwrap();
    assert_eq!(summary.payload["terminal_status"], "budget_exhausted");
}

fn text_occurrences(req: &rho_engine::provider::adapter::ModelCompletionRequest, needle: &str) -> usize {
    let encoded = serde_json::to_string(&req.messages).unwrap();
    encoded.matches(needle).count()
}

async fn run_follow_up_turns(engine: &rho::engine::AgentEngine) {
    let presenter = Arc::new(StructuredPresenter::stdout());
    engine
        .run_turn(TurnRequest::new("one"), presenter.clone())
        .await
        .unwrap();
    engine.run_turn(TurnRequest::new("two"), presenter).await.unwrap();
}

async fn run_resumed_session(dir: &Path, sid: &str) -> usize {
    let resumed_store = SessionManager::new(&dir.join("sessions"), Some(sid)).unwrap();
    let resumed_model = Arc::new(MockAdapter::text("resumed"));
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
        .run_turn(TurnRequest::new("three"), Arc::new(StructuredPresenter::stdout()))
        .await
        .unwrap();
    text_occurrences(&resumed_model.requests()[0], "one")
}

#[tokio::test]
async fn agent_eval_session_follow_up_and_resume() {
    let dir = temp_dir("session_resume");
    std::fs::create_dir_all(&dir).unwrap();
    let first_model = Arc::new(MockAdapter::default());
    first_model.add_turn(vec![Ok(ModelStreamEvent::Text("first".to_string()))]);
    first_model.add_turn(vec![Ok(ModelStreamEvent::Text("follow-up".to_string()))]);
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
    let cleared_model = Arc::new(MockAdapter::text("fresh"));
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
            Arc::new(StructuredPresenter::stdout()),
        )
        .await
        .unwrap();
    let msgs = &cleared_model.requests()[0].messages;
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].text().unwrap().contains("fresh prompt"));
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

    let model = Arc::new(MockAdapter::text("done"));
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
