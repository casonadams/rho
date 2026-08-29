use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use super::{AgentEngine, ExtensionRegistry};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentBackend;
use crate::engine::provider::{CredentialStrategy, ModelRequest, ProviderFactory};
use crate::error::Result;
use crate::plugin::provider::{ActiveProvider, ProviderRegistry};
use crate::session::SessionManager;
use std::path::PathBuf;

/// Constructs an engine while keeping provider, session, and plugin setup out
/// of the run coordinator.
pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    base_dir: Option<PathBuf>,
}

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
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
        let registry = ProviderRegistry::load(&self.config).await?;
        let active_provider = registry.get(&self.config.provider)?.clone();
        let mut secrets = self.auth_store.secret_values();
        if let ActiveProvider::Builtin(provider) = &active_provider
            && provider.id().credential_strategy() == CredentialStrategy::ApiKey
            && let Some(key) = self.auth_store.get_key(provider.id().as_str())?
        {
            secrets.push(key);
        }
        session_manager.add_secrets(secrets)?;
        let active_tools = crate::plugin::tool_dispatch::ActiveToolSet::load(&self.config, &base_dir).await?;
        let backend = match active_provider {
            ActiveProvider::Builtin(provider) => {
                let model = ProviderFactory::create_model_for(
                    provider.id(),
                    ModelRequest {
                        model: &self.config.model,
                        config_dir: &self.config.config_dir,
                    },
                    &self.auth_store,
                )?;
                AgentBackend::Rig(Box::new(super::runtime::build_coding_agent(
                    model,
                    &self.config,
                    CodingRuntime {
                        base_dir: &base_dir,
                        memory: session_manager.clone(),
                        active_tools: Some(active_tools),
                    },
                )?))
            }
            external @ ActiveProvider::External { .. } => {
                let credential = self
                    .auth_store
                    .scoped_credential(&external.credential_scope())
                    .map(|(_, credential)| credential);
                AgentBackend::External {
                    provider: external.capability().ok_or_else(|| {
                        crate::error::AppError::Provider("external provider capability is unavailable".to_string())
                    })?,
                    tools: std::sync::Arc::new(active_tools),
                    credential,
                }
            }
        };
        let extension_registry = ExtensionRegistry::new();
        Ok(AgentEngine {
            config: self.config.clone(),
            session_manager,
            extension_registry,
            backend,
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(self.config.context_limit),
            run_tracker: super::metrics::RunTracker::default(),
        })
    }
}
