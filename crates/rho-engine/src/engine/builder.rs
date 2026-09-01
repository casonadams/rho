use super::AgentEngine;
use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use rho_core::config::Config;
use rho_core::error::Result;
use rho_core::session::SessionManager;
use std::path::PathBuf;
use std::sync::Arc;

pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    base_dir: Option<PathBuf>,
    rig_tools: Option<Vec<rig::tool::DynamicTool>>,
}

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
            rig_tools: None,
            config,
            auth_store,
            resume_id: None,
            session_manager: None,
            base_dir: None,
        }
    }

    pub fn resume(mut self, resume_id: Option<&str>) -> Self {
        self.resume_id = resume_id.map(str::to_owned);
        self
    }

    pub fn tools(mut self, rig_tools: Vec<rig::tool::DynamicTool>) -> Self {
        self.rig_tools = Some(rig_tools);
        self
    }

    pub fn session(mut self, session_manager: SessionManager) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    pub fn base_dir(mut self, base_dir: PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    pub async fn build(self) -> Result<AgentEngine> {
        let base_dir = self.base_dir.unwrap_or(std::env::current_dir()?);
        let session_manager = match self.session_manager {
            Some(session) => session,
            None => SessionManager::new_with_secrets(
                &self.config.sessions_dir,
                self.resume_id.as_deref(),
                self.auth_store.secret_values(),
            )?,
        };

        let model = crate::provider::ProviderFactory::create_model(
            &self.config.provider,
            &self.config.model,
            &self.auth_store,
        )?;

        let agent = super::runtime::build_coding_agent(
            model,
            &self.config,
            CodingRuntime {
                base_dir: &base_dir,
                memory: session_manager.clone(),
                built_in_tools: self.rig_tools.clone(),
            },
        )?;

        Ok(AgentEngine {
            config: self.config.clone(),
            session_manager,
            agent: Box::new(agent),
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(self.config.context_limit),
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
        })
    }
}
