use super::AgentEngine;
use super::provider::registry::{ActiveProvider, ProviderRegistry};
use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use crate::engine::AgentBackend;
use crate::engine::provider::{CredentialStrategy, ModelRequest, ProviderFactory};
use rho_core::config::Config;
use rho_core::dispatch::NeutralToolExecutor;
use rho_core::error::Result;
use rho_core::session::SessionManager;
use rho_sdk::contract::{ContextCapability, LifecycleCapability};
use std::path::PathBuf;
use std::sync::Arc;

/// Constructs an engine while keeping provider, session, and plugin setup out
/// of the run coordinator.
pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    session_approvals: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
    base_dir: Option<PathBuf>,
    custom_provider: Option<Arc<dyn rho_sdk::contract::ProviderCapability>>,
    rig_tools: Option<Vec<rig::tool::DynamicTool>>,
    neutral_executor: Option<std::sync::Arc<dyn NeutralToolExecutor>>,
    contexts: Vec<Arc<dyn ContextCapability>>,
    lifecycles: Vec<Arc<dyn LifecycleCapability>>,
}

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
            rig_tools: None,
            neutral_executor: None,
            custom_provider: None,
            config,
            auth_store,
            resume_id: None,
            session_manager: None,
            session_approvals: None,
            base_dir: None,
            contexts: Vec::new(),
            lifecycles: Vec::new(),
        }
    }

    pub fn resume(mut self, resume_id: Option<&str>) -> Self {
        self.resume_id = resume_id.map(str::to_owned);
        self
    }

    /// Inject the platform's assembled tools (rig adapters + neutral executor).
    pub fn tool_assembly(
        mut self,
        rig_tools: Vec<rig::tool::DynamicTool>,
        neutral_executor: std::sync::Arc<dyn NeutralToolExecutor>,
    ) -> Self {
        self.rig_tools = Some(rig_tools);
        self.neutral_executor = Some(neutral_executor);
        self
    }

    pub fn session(mut self, session_manager: SessionManager) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    pub fn session_approvals(
        mut self,
        session_approvals: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) -> Self {
        self.session_approvals = Some(session_approvals);
        self
    }

    pub fn base_dir(mut self, base_dir: PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    pub fn provider(mut self, provider: Arc<dyn rho_sdk::contract::ProviderCapability>) -> Self {
        self.custom_provider = Some(provider);
        self
    }

    pub fn contexts(mut self, contexts: Vec<Arc<dyn ContextCapability>>) -> Self {
        self.contexts = contexts;
        self
    }

    pub fn lifecycles(mut self, lifecycles: Vec<Arc<dyn LifecycleCapability>>) -> Self {
        self.lifecycles = lifecycles;
        self
    }

    pub async fn build_with_assembly(
        self,
        assembly: (Vec<rig::tool::DynamicTool>, std::sync::Arc<dyn NeutralToolExecutor>),
    ) -> Result<AgentEngine> {
        self.tool_assembly(assembly.0, assembly.1).build().await
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
        let backend = if let Some(provider) = self.custom_provider {
            AgentBackend::External {
                provider,
                tools: self
                    .neutral_executor
                    .clone()
                    .ok_or_else(|| rho_core::error::AppError::Plugin("tool platform was not injected".to_string()))?,
                credential: None,
            }
        } else {
            match active_provider {
                ActiveProvider::Builtin(provider) if provider.id() == rho_core::provider::ProviderId::Antigravity => {
                    let antigravity_cap: Arc<dyn rho_sdk::contract::ProviderCapability> = Arc::new(
                        rho_plugin_providers::antigravity::AntigravityProvider::new(self.config.config_dir.clone()),
                    );
                    AgentBackend::External {
                        provider: antigravity_cap,
                        tools: self.neutral_executor.clone().ok_or_else(|| {
                            rho_core::error::AppError::Plugin("tool platform was not injected".to_string())
                        })?,
                        credential: None,
                    }
                }
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
                            built_in_tools: self.rig_tools.clone(),
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
                            rho_core::error::AppError::Provider(
                                "external provider capability is unavailable".to_string(),
                            )
                        })?,
                        tools: self.neutral_executor.clone().ok_or_else(|| {
                            rho_core::error::AppError::Plugin("tool platform was not injected".to_string())
                        })?,
                        credential,
                    }
                }
            }
        };
        let session_approvals = self
            .session_approvals
            .unwrap_or_else(|| std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())));
        Ok(AgentEngine {
            config: self.config.clone(),
            session_manager,
            session_approvals,
            backend,
            contexts: self.contexts,
            lifecycles: self.lifecycles,
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(self.config.context_limit),
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
        })
    }
}
