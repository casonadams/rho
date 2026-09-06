pub use crate::repeat;
pub use crate::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook, normalized_call_key};
use std::path::Path;
pub use tracking::{SessionUsageTotals, SpeedTracker};
pub mod builder;
pub use builder::{AgentEngineBuilder, create_engine_model};
pub mod compactor;
pub mod context;
mod display;
pub mod metrics;
mod model;
mod quota;
pub mod runner;
pub mod runtime;
pub mod tracking;

pub use compactor::CompactionStats;

#[cfg(test)]
mod tests;

pub mod eval;

use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionManager;
use rig::agent::Agent;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub(crate) tools: Vec<rig::tool::DynamicTool>,
    pub(crate) tool_names: Arc<std::sync::RwLock<Vec<String>>>,
    pub(crate) plugins: Vec<Arc<dyn crate::plugin::RhoPlugin>>,
    pub(crate) agent: Arc<tokio::sync::RwLock<Agent>>,
    pub(crate) usage: UsageTracker,
    pub(crate) quota: QuotaTracker,
    pub(crate) context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
    pub(crate) project_context: Arc<tokio::sync::Mutex<Option<(std::path::PathBuf, context::ProjectContext)>>>,
    pub(crate) auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    pub(crate) model: Option<rig::agent::ModelHandle>,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        builder::AgentEngineBuilder::new(config, auth_store)
            .resume(resume_id)
            .build()
            .await
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tool_names.read().unwrap().clone()
    }

    pub fn shared_auth_store(&self) -> Arc<tokio::sync::Mutex<AuthStore>> {
        Arc::clone(&self.auth_store)
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        let base_dir = std::env::current_dir()?;
        let rebuilt = builder::AgentEngineBuilder::new(config, auth_store)
            .session(self.session_manager.clone())
            .base_dir(base_dir)
            .build()
            .await?;
        rebuilt.refresh_quota().await;
        Ok(rebuilt)
    }

    fn build_cached_context_dirs<'a>(
        &'a self,
        tools: &'a [String],
        home_dir: Option<&'a Path>,
    ) -> context::ContextDirs<'a> {
        context::ContextDirs {
            config_dir: Some(&self.config.config_dir),
            home_dir,
            system_prompt: self.config.system_prompt.as_deref(),
            append_system_prompt: self.config.append_system_prompt.as_deref(),
            active_tools: Some(tools),
            no_context_files: self.config.no_context_files,
        }
    }

    pub async fn project_context(&self) -> Result<context::ProjectContext> {
        let cwd = std::env::current_dir()?;
        let mut cache = self.project_context.lock().await;
        if cache.as_ref().map(|(dir, _)| dir.as_path()) != Some(cwd.as_path()) {
            let tools = self.tool_names();
            let home = context::resolve_home_dir();
            let dirs = self.build_cached_context_dirs(&tools, home.as_deref());
            *cache = Some((
                cwd.clone(),
                context::ProjectContext::discover_with_dirs(&cwd, dirs).await,
            ));
        }
        let Some((_, cached)) = cache.as_mut() else {
            return Err(AppError::Other(anyhow::anyhow!("project context cache unavailable")));
        };
        cached.refresh_runtime_state().await;
        Ok(cached.clone())
    }

    pub async fn instruction_files(&self) -> Vec<String> {
        self.project_context()
            .await
            .map(|ctx| ctx.instruction_files.into_iter().map(|(path, _)| path).collect())
            .unwrap_or_default()
    }

    pub async fn activate_path_instructions(&self, path: &std::path::Path) {
        let mut cache = self.project_context.lock().await;
        if let Some((_, cached)) = cache.as_mut() {
            cached.activate_path_instructions_async(path).await;
        }
    }
}
