use std::str::FromStr;

use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;

use super::AgentEngine;
use super::builder;
use super::runtime;
use super::tracking::ContextTracker;

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
        if let Ok(provider_id) = ProviderId::from_str(self.config.provider.trim()) {
            let mut store = self.auth_store.lock().await;
            let _ = store.get_key(provider_id.as_str()).await;
        }

        let model_handle = self.build_model_handle(&self.config).await?;
        self.model = Some(model_handle.clone());

        let context_limit = match self.config.context_limit {
            Some(limit) => Some(limit),
            None if matches!(self.config.provider.as_str(), "local" | "ollama" | "ollama-cloud") => {
                let store = crate::provider::ModelStore::load(self.config.config_dir.join("models-store.json"));
                let keys: &[&str] = if self.config.provider == "ollama-cloud" {
                    &["ollama-cloud"]
                } else {
                    &["local", "ollama"]
                };
                store.context_tokens(keys, &self.config.model)
            }
            None => None,
        };
        self.context = ContextTracker::new(context_limit);

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
        self.context.limit_for(&self.config.model)
    }
}
