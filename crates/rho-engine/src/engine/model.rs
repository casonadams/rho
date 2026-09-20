use rho_harness_core::config::Config;
use rho_harness_core::error::Result;

use super::AgentEngine;
use super::builder;
use super::tracking::ContextTracker;

pub(crate) fn resolve_context_limit(config: &Config) -> Option<usize> {
    if let Some(limit) = config.context_limit {
        return Some(limit);
    }
    let store = crate::provider::ModelStore::load(config.config_dir.join("models-store.json"));
    if matches!(config.provider.as_str(), "local" | "ollama") {
        store.context_tokens(&["local", "ollama"], &config.model)
    } else {
        store.context_tokens(&[&config.provider], &config.model)
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
        let model_handle = self.build_model_handle(&self.config).await?;
        self.model = Some(model_handle.clone());
        self.context = ContextTracker::new(resolve_context_limit(&self.config));

        self.agent.write().await.set_model_handle(model_handle);
        self.force_refresh_quota();
        Ok(())
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model, &self.config.provider)
    }
}
