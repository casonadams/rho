use crate::auth::AuthStore;
use std::str::FromStr;

use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;

use super::AgentEngine;
use super::builder;
use super::runtime;
use super::tracking::ContextTracker;

pub(crate) fn resolve_context_limit(config: &Config) -> Option<usize> {
    if let Some(limit) = config.context_limit {
        return Some(limit);
    }
    if matches!(config.provider.as_str(), "local" | "ollama" | "ollama-cloud") {
        let store = crate::provider::ModelStore::load(config.config_dir.join("models-store.json"));
        let keys: &[&str] = if config.provider == "ollama-cloud" {
            &["ollama-cloud"]
        } else {
            &["local", "ollama"]
        };
        store.context_tokens(keys, &config.model)
    } else {
        None
    }
}

async fn refresh_oauth_key_if_applicable(provider: &str, auth_store: &tokio::sync::Mutex<AuthStore>) {
    if let Ok(provider_id) = ProviderId::from_str(provider.trim()) {
        let mut store = auth_store.lock().await;
        let _ = store.get_key(provider_id.as_str()).await;
    }
}

impl AgentEngine {
    pub async fn build_model_handle(&self, config: &Config) -> Result<rig::agent::ModelHandle> {
        let auth_store = self.auth_store.lock().await;
        builder::create_engine_model(config, &auth_store, Some(self.auth_store.clone()))
    }

    pub async fn switch_model(&mut self, model: &str, provider: &str) -> Result<()> {
        self.config.model = model.to_string();
        self.config.provider = provider.to_string();
        self.update_model().await
    }

    pub async fn update_model(&mut self) -> Result<()> {
        refresh_oauth_key_if_applicable(&self.config.provider, &self.auth_store).await;
        let model_handle = self.build_model_handle(&self.config).await?;
        self.model = Some(model_handle.clone());
        self.context = ContextTracker::new(resolve_context_limit(&self.config));

        let base_dir = std::env::current_dir()?;
        let new_agent = runtime::build_coding_agent(
            model_handle,
            &self.config,
            runtime::CodingRuntime {
                base_dir: &base_dir,
                memory: self.session_manager.clone(),
                built_in_tools: Some(self.tools.clone()),
            },
        )?;
        *self.agent.write().await = new_agent;
        self.spawn_refresh_quota();
        Ok(())
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model, &self.config.provider)
    }
}
