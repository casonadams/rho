//! Test-only helpers for constructing a real `AgentEngine` backed by a mock LLM.
//!
//! These exist so individual tests can express scenarios as a sequence of
//! `MockStreamEvent` turns without dragging rig or the runtime into each
//! test file. Nothing here is reachable from production code; the whole
//! module is gated `#[cfg(test)]`.

use crate::engine::AgentEngine;
use crate::engine::metrics::RunTracker;
use crate::engine::runtime::{CodingRuntime, build_coding_agent};
use rho_harness_core::config::Config;
use rho_harness_core::session::SessionManager;
use rig::agent::ModelHandle;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::path::{Path, PathBuf};

pub struct MockEngineConfig<'a> {
    pub base_dir: &'a Path,
    /// The caller's application config; `sessions_dir` is overridden to point
    /// at the harness's own session tree.
    pub app_config: Config,
    pub session_manager: Option<SessionManager>,
    pub built_in_tools: Option<Vec<rig::tool::DynamicTool>>,
}

pub fn mock_engine(model: MockCompletionModel, config: MockEngineConfig<'_>) -> AgentEngine {
    let cfg = MockEngineConfig {
        session_manager: Some(match config.session_manager {
            Some(session_manager) => session_manager,
            None => {
                let sessions = config.base_dir.join("sessions");
                SessionManager::new(&sessions, None).unwrap()
            }
        }),
        ..config
    };
    mock_engine_with_session(model, cfg)
}

pub fn mock_engine_with_session(model: MockCompletionModel, config: MockEngineConfig<'_>) -> AgentEngine {
    let app_config = Config {
        sessions_dir: config.base_dir.join("sessions"),
        ..config.app_config.clone()
    };
    let session_manager = match config.session_manager {
        Some(session_manager) => session_manager,
        None => SessionManager::new(&config.base_dir.join("sessions"), None).unwrap(),
    };
    let agent = build_coding_agent(
        ModelHandle::new(model),
        &app_config,
        CodingRuntime {
            base_dir: config.base_dir,
            memory: session_manager.clone(),
            built_in_tools: config.built_in_tools.clone(),
        },
    )
    .unwrap();
    let tool_names = config
        .built_in_tools
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|tool| tool.name().to_string())
        .collect();
    let base_tools = config.built_in_tools.clone().unwrap_or_default();
    AgentEngine {
        config: app_config,
        session_manager,
        tool_names: std::sync::Arc::new(std::sync::RwLock::new(tool_names)),
        plugins: Vec::new(),
        agent: std::sync::Arc::new(tokio::sync::RwLock::new(agent)),
        usage: crate::engine::tracking::UsageTracker::default(),
        quota: crate::engine::tracking::QuotaTracker::default(),
        context: crate::engine::tracking::ContextTracker::new(None),
        run_tracker: RunTracker::default(),
        project_context: std::sync::Arc::default(),
        auth_store: std::sync::Arc::new(tokio::sync::Mutex::new(crate::auth::AuthStore::default())),
        base_tools,
        base_dir: std::path::PathBuf::from("."),
        model: None,
        mcp_loader: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
    }
}

pub fn final_event(usage: Usage) -> MockStreamEvent {
    MockStreamEvent::final_response(usage)
}

pub fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agent_eval_{label}_{}", uuid::Uuid::new_v4()))
}
