pub mod context;
pub mod metrics;
pub mod provider;
pub mod runner;
pub mod runtime;

#[cfg(test)]
mod eval;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::{CredentialStrategy, ModelRequest, ProviderFactory, ProviderId};
use crate::error::Result;
use crate::session::SessionManager;
use rig::agent::Agent;
use rig::completion::Usage;
use runtime::CodingRuntime;
use std::sync::Mutex;

use metrics::RunTracker;

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    agent: Agent,
    last_usage: Mutex<Option<Usage>>,
    run_tracker: RunTracker,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        let session_manager =
            SessionManager::new_with_secrets(&config.sessions_dir, resume_id, auth_store.secret_values())?;
        Self::with_session(config, auth_store, session_manager)
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        Self::with_session(config, auth_store, self.session_manager.clone())
    }

    fn with_session(config: Config, auth_store: AuthStore, session_manager: SessionManager) -> Result<Self> {
        let provider = config.provider.parse::<ProviderId>()?;
        let mut secrets = auth_store.secret_values();
        if provider.credential_strategy() == CredentialStrategy::ApiKey
            && let Some(key) = auth_store.get_key(provider.as_str())?
        {
            secrets.push(key);
        }
        session_manager.add_secrets(secrets)?;
        let base_dir = std::env::current_dir()?;
        let request = ModelRequest {
            model: &config.model,
            config_dir: &config.config_dir,
        };
        let model = ProviderFactory::create_model_for(provider, request, &auth_store)?;
        let agent = runtime::build_coding_agent(
            model,
            &config,
            CodingRuntime {
                base_dir: &base_dir,
                memory: session_manager.clone(),
            },
        )?;
        Ok(Self {
            config,
            session_manager,
            agent,
            last_usage: Mutex::new(None),
            run_tracker: RunTracker::default(),
        })
    }

    pub fn context_usage_percent(&self) -> Option<usize> {
        let usage = self.last_usage.lock().ok().and_then(|usage| *usage)?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.config.context_limit?;
        Some(((usage.input_tokens as usize * 100) / limit).min(100))
    }

    pub fn context_usage_display(&self) -> String {
        if let Some(value) = self.context_usage_percent() {
            return format!("{value}%");
        }
        self.last_usage.lock().ok().and_then(|usage| *usage).map_or_else(
            || "usage unavailable".to_string(),
            |usage| format!("{} input tokens", usage.input_tokens),
        )
    }

    fn record_usage(&self, usage: Usage) {
        if let Ok(mut current) = self.last_usage.lock() {
            *current = usage.has_values().then_some(usage);
        }
    }
}
