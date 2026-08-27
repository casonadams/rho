//! Test-only helpers for constructing a real `AgentEngine` backed by a mock LLM.
//!
//! These exist so individual tests can express scenarios as a sequence of
//! `MockStreamEvent` turns without dragging rig or the runtime into each
//! test file. Nothing here is reachable from production code; the whole
//! module is gated `#[cfg(test)]`.

use crate::config::Config;
use crate::engine::AgentEngine;
use crate::engine::metrics::RunTracker;
use crate::engine::runtime::{CodingRuntime, build_coding_agent};
use crate::session::SessionManager;
use rig::agent::ModelHandle;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::path::{Path, PathBuf};

pub(super) struct MockEngineConfig<'a> {
    pub(super) base_dir: &'a Path,
    pub(super) max_turns: usize,
    pub(super) session_manager: SessionManager,
}

pub(super) fn mock_engine(model: MockCompletionModel, base_dir: &Path, max_turns: usize) -> AgentEngine {
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

pub(super) fn mock_engine_with_session(model: MockCompletionModel, config: MockEngineConfig<'_>) -> AgentEngine {
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
            active_tools: None,
        },
    )
    .unwrap();
    AgentEngine {
        config: app_config,
        session_manager: config.session_manager,
        extension_registry: crate::plugin::ExtensionRegistry::new(),
        agent,
        usage: crate::engine::tracking::UsageTracker::default(),
        quota: crate::engine::tracking::QuotaTracker::default(),
        context: crate::engine::tracking::ContextTracker::new(None),
        run_tracker: RunTracker::default(),
    }
}

pub(super) fn final_event(usage: Usage) -> MockStreamEvent {
    MockStreamEvent::final_response(usage)
}

pub(super) fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agent_eval_{label}_{}", uuid::Uuid::new_v4()))
}
