//! Test-only helpers for constructing a real `AgentEngine` backed by a mock LLM.

use crate::engine::AgentEngine;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::RunTracker;
use crate::provider::adapter::ModelAdapter;
#[cfg(test)]
use crate::test_support::{TestDir, temp_dir};
use crate::tools::BuiltinTools;
use rho_harness_core::config::Config;
use rho_harness_core::session::SessionManager;
use std::path::Path;
use std::sync::Arc;

pub struct MockEngineConfig<'a> {
    pub base_dir: &'a Path,
    pub app_config: Config,
    pub session_manager: Option<SessionManager>,
    pub built_in_tools: Option<BuiltinTools>,
}

impl<'a> MockEngineConfig<'a> {
    /// An engine under `base_dir` with `app_config`, its own session, and no tools.
    pub fn new(base_dir: &'a Path, app_config: Config) -> Self {
        Self {
            base_dir,
            app_config,
            session_manager: None,
            built_in_tools: None,
        }
    }
}

#[cfg(test)]
pub fn mock_engine_in_dir(label: &str, model: Arc<dyn ModelAdapter>) -> (AgentEngine, TestDir) {
    let dir = temp_dir(label);
    let config = Config {
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let engine = mock_engine(model, MockEngineConfig::new(&dir, config));
    (engine, dir)
}

pub fn mock_engine(model: Arc<dyn ModelAdapter>, config: MockEngineConfig<'_>) -> AgentEngine {
    let app_config = Config {
        sessions_dir: config.base_dir.join("sessions"),
        ..config.app_config.clone()
    };
    let session_manager = resolve_mock_session_manager(&config);
    let model_handle: ModelHandle = model;
    let BuiltinTools { tools, runtime } = config.built_in_tools.unwrap_or_else(|| BuiltinTools {
        tools: Vec::new(),
        runtime: crate::tools::subagent::SubagentRuntime::default(),
    });
    let tool_names = tools.iter().map(|tool| tool.name().to_string()).collect();

    AgentEngine {
        config: app_config,
        base_dir: config.base_dir.to_path_buf(),
        session_manager,
        tool_names: std::sync::Arc::new(std::sync::RwLock::new(tool_names)),
        tools: std::sync::Arc::new(tokio::sync::RwLock::new(tools)),
        usage: crate::engine::tracking::UsageTracker::default(),
        quota: crate::engine::tracking::QuotaTracker::default(),
        quota_endpoints: crate::engine::quota_refresh::QuotaEndpoints::all_at("http://127.0.0.1:9"),
        title_tracker: crate::engine::title::TitleTracker::default(),
        context: crate::engine::tracking::ContextTracker::new(),
        run_tracker: RunTracker::default(),
        project_context: std::sync::Arc::default(),
        router: crate::provider::ModelRouter::new(
            std::sync::Arc::new(tokio::sync::Mutex::new(crate::auth::AuthStore::default())),
            crate::provider::ModelCatalog::default(),
        ),
        model: Some(model_handle),
        compaction_summaries: crate::engine::compactor::CompactionSummaries::Heuristic,
        demotion_hook: None,
        agent_monitor: rho_harness_core::agents::AgentMonitor::default(),
        subagent_runtime: runtime,
        turn_notes: crate::engine::TurnNotes::default(),
        prune_memory: std::sync::Mutex::default(),
        worktrees: None,
        pending_relocation: std::sync::Arc::default(),
        workflows: None,
        subagent_models: None,
        mcp: crate::engine::EngineMcp::default(),
        ollama_host: crate::ollama::local::local_host(),
        presenter: None,
    }
}

fn resolve_mock_session_manager(config: &MockEngineConfig<'_>) -> SessionManager {
    match config.session_manager.clone() {
        Some(mgr) => mgr,
        None => SessionManager::new(&config.base_dir.join("sessions"), None).unwrap(),
    }
}
