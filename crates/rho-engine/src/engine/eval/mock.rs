//! Test-only helpers for constructing a real `AgentEngine` backed by a mock LLM.

use crate::engine::AgentEngine;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::RunTracker;
use crate::provider::adapter::ModelAdapter;
use crate::tools::EngineTool;
use rho_harness_core::config::Config;
use rho_harness_core::session::SessionManager;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct MockEngineConfig<'a> {
    pub base_dir: &'a Path,
    pub app_config: Config,
    pub session_manager: Option<SessionManager>,
    pub built_in_tools: Option<Vec<Arc<dyn EngineTool>>>,
}

pub fn mock_engine(model: Arc<dyn ModelAdapter>, config: MockEngineConfig<'_>) -> AgentEngine {
    let app_config = Config {
        sessions_dir: config.base_dir.join("sessions"),
        ..config.app_config.clone()
    };
    let session_manager = resolve_mock_session_manager(&config);
    let model_handle: ModelHandle = model;
    let tools = config.built_in_tools.unwrap_or_default();
    let tool_names = tools.iter().map(|tool| tool.name().to_string()).collect();

    AgentEngine {
        config: app_config,
        base_dir: config.base_dir.to_path_buf(),
        session_manager,
        tool_names: std::sync::Arc::new(std::sync::RwLock::new(tool_names)),
        tools: std::sync::Arc::new(tokio::sync::RwLock::new(tools)),
        usage: crate::engine::tracking::UsageTracker::default(),
        quota: crate::engine::tracking::QuotaTracker::default(),
        context: crate::engine::tracking::ContextTracker::new(None),
        run_tracker: RunTracker::default(),
        project_context: std::sync::Arc::default(),
        auth_store: std::sync::Arc::new(tokio::sync::Mutex::new(crate::auth::AuthStore::default())),
        model: Some(model_handle),
        compactor_model: None,
        demotion_hook: None,
    }
}

pub fn mock_engine_with_session(model: Arc<dyn ModelAdapter>, config: MockEngineConfig<'_>) -> AgentEngine {
    mock_engine(model, config)
}

fn resolve_mock_session_manager(config: &MockEngineConfig<'_>) -> SessionManager {
    match config.session_manager.clone() {
        Some(mgr) => mgr,
        None => SessionManager::new(&config.base_dir.join("sessions"), None).unwrap(),
    }
}

pub fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agent_eval_{label}_{}", uuid::Uuid::new_v4()))
}
