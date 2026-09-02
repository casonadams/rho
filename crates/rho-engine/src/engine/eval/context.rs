//! Long-context comparison harness: bounded vs. unbounded history.
//!
//! This module is separate from [`harness`](super::harness) because it
//! constructs its own `AgentEngine` with custom memory wiring
//! (`context_memory`) rather than going through the mock. The tests in
//! [`super::tests`] that exercise long-context correlation rely on
//! the helpers defined here.

use super::mock::final_event;
use crate::engine::AgentEngine;
use crate::engine::metrics::{RunTracker, TerminalStatus};
use crate::engine::runner::{TurnOutput, TurnRequest};
use rho_harness_core::config::Config;
use rho_harness_core::session::SessionManager;
use rho_harness_core::session::context::{context_memory, model_visible_bytes};
use rig::agent::ModelHandle;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextComparisonReport {
    pub scenario: &'static str,
    pub before: ContextEvaluation,
    pub after: ContextEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEvaluation {
    pub model_visible_messages: usize,
    pub model_visible_bytes: usize,
    pub input_tokens: Option<u64>,
    pub success: bool,
    pub terminal_status: TerminalStatus,
    pub turns: usize,
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub tool_denials: usize,
    pub usage_available: bool,
}

pub struct ContextEvaluationInput<'a> {
    pub base_dir: &'a Path,
    pub history: &'a [Message],
    pub bounded: bool,
    pub usage: Usage,
}

pub async fn run_context_evaluation(input: ContextEvaluationInput<'_>) -> ContextEvaluation {
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
    let agent = rig::agent::AgentBuilder::from_model_handle(ModelHandle::new(model.clone()))
        .memory(memory)
        .record_content_telemetry(false)
        .build();
    let engine = AgentEngine {
        config,
        session_manager: store,
        tool_names: Vec::new(),
        plugins: Vec::new(),
        agent: Box::new(agent),
        usage: crate::engine::tracking::UsageTracker::default(),
        quota: crate::engine::tracking::QuotaTracker::default(),
        context: crate::engine::tracking::ContextTracker::new(None),
        run_tracker: RunTracker::default(),
        project_context: std::sync::Arc::default(),
    };
    let TurnOutput { metrics, usage, .. } = engine
        .run_turn(TurnRequest::new("continue"), super::presenter::presenter())
        .await
        .unwrap();
    let visible = &model.requests()[0].chat_history;
    ContextEvaluation {
        model_visible_messages: visible.len(),
        model_visible_bytes: model_visible_bytes(visible),
        input_tokens: usage.map(|usage| usage.input_tokens),
        success: metrics.success,
        terminal_status: metrics.terminal_status,
        turns: metrics.model_turns,
        tool_calls: metrics.tool_calls,
        tool_errors: metrics.tool_errors,
        tool_denials: metrics.tool_denials,
        usage_available: metrics.usage_available,
    }
}

pub fn long_context_history() -> Vec<Message> {
    (0..30)
        .flat_map(|index| {
            [
                Message::user(format!("historical request {index}: {}", "context ".repeat(30))),
                Message::assistant(format!("historical response {index}: {}", "result ".repeat(30))),
            ]
        })
        .collect()
}

pub async fn context_comparison(base_dir: &Path, before_usage: Usage, after_usage: Usage) -> ContextComparisonReport {
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
