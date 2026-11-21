use rho_harness_core::config::Config;
use rho_harness_core::error::Result;

use super::AgentEngine;
use super::builder;
use super::tracking::{ContextLimitSource, ContextTracker};
use crate::provider::{LOCAL_OLLAMA_PROVIDER, ModelCatalog};

impl AgentEngine {
    pub async fn build_model_handle(&self, config: &Config) -> Result<crate::engine::compactor::llm::ModelHandle> {
        let auth_store = self.auth_store.lock().await;
        builder::create_engine_model(config, &auth_store, self.auth_store.clone())
    }

    pub async fn switch_model(&mut self, model: &str, provider: &str) -> Result<()> {
        self.config.model = model.to_string();
        self.config.provider = provider.to_string();
        self.update_model().await
    }

    pub async fn update_model(&mut self) -> Result<()> {
        let (spec_p, _) = rho_harness_core::provider::parse_model_spec(&self.config.model);
        if !spec_p.is_empty() {
            self.config.provider = spec_p;
        }
        let model_handle = self.build_model_handle(&self.config).await?;
        self.model = Some(model_handle);
        self.context = ContextTracker::new(self.config.context_limit, self.model_catalog().clone());

        self.force_refresh_quota();
        Ok(())
    }

    pub fn model_catalog(&self) -> &ModelCatalog {
        self.context.catalog()
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model, &self.config.provider)
    }

    pub fn context_limit_source(&self) -> ContextLimitSource {
        self.context.resolve_limit(&self.config.model, &self.config.provider).1
    }

    /// Why rho could not read the local Ollama server's allocated window after the last turn.
    pub fn context_probe_error(&self) -> Option<String> {
        self.context.probe_error()
    }

    /// Ollama only reports the window it allocated while the model is loaded, which is guaranteed
    /// right after a turn; the OpenAI-compatible API cannot request a window size.
    pub(crate) fn spawn_local_context_probe(&self) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        if model.provider_name() != LOCAL_OLLAMA_PROVIDER || self.config.context_limit.is_some() {
            return;
        }
        let probe = LocalContextProbe {
            host: self.ollama_host.clone(),
            model: model.model_name().to_string(),
            catalog_provider: self.config.provider.clone(),
            catalog_model: self.config.model.clone(),
            context: self.context.clone(),
        };
        tokio::spawn(probe.run());
    }
}

struct LocalContextProbe {
    host: String,
    model: String,
    catalog_provider: String,
    catalog_model: String,
    context: ContextTracker,
}

impl LocalContextProbe {
    async fn run(self) {
        match crate::ollama::local::loaded_context_length(&self.host, &self.model).await {
            Ok(Some(tokens)) => {
                self.context
                    .catalog()
                    .record_observed_context(&self.catalog_provider, &self.catalog_model, tokens);
                self.context.set_probe_error(None);
            }
            Ok(None) => self.context.set_probe_error(None),
            Err(error) => self.context.set_probe_error(Some(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::{FinishReason, MockAdapter, ModelStreamEvent};
    use crate::provider::{DiscoveredModel, ProviderModels};
    use crate::test_support::serve_http;

    const MODEL: &str = "inkling-2-mini:latest";

    fn catalog_with_trained_window() -> ModelCatalog {
        let catalog = ModelCatalog::default();
        let model = DiscoveredModel {
            context_tokens: Some(1_048_576),
            ..DiscoveredModel::new(MODEL, "inkling", "local", "1M ctx")
        };
        catalog.set("local", ProviderModels::Ready(vec![model]));
        catalog
    }

    fn ps_body(context_length: usize) -> String {
        serde_json::json!({ "models": [{ "name": MODEL, "context_length": context_length }] }).to_string()
    }

    fn probe(host: String, context: &ContextTracker) -> LocalContextProbe {
        LocalContextProbe {
            host,
            model: "inkling-2-mini".to_string(),
            catalog_provider: "local".to_string(),
            catalog_model: MODEL.to_string(),
            context: context.clone(),
        }
    }

    #[tokio::test]
    async fn completed_local_turn_replaces_trained_window_with_allocated_one() {
        let dir = tempfile::tempdir().unwrap();
        let model = MockAdapter::new("inkling-2-mini", LOCAL_OLLAMA_PROVIDER);
        model.add_turn(vec![
            Ok(ModelStreamEvent::Text("done".into())),
            Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
        ]);
        let config = Config {
            provider: "ollama".to_string(),
            model: MODEL.to_string(),
            ..Config::default()
        };
        let mut engine = crate::engine::eval::mock::mock_engine(
            std::sync::Arc::new(model),
            crate::engine::eval::mock::MockEngineConfig {
                base_dir: dir.path(),
                app_config: config,
                session_manager: None,
                built_in_tools: None,
            },
        );
        let catalog = catalog_with_trained_window();
        engine.context = ContextTracker::new(None, catalog.clone());
        engine.ollama_host = serve_http(vec![("200 OK", ps_body(262_144))]).await;
        assert_eq!(engine.context_limit(), Some(1_048_576));
        let mut changes = catalog.subscribe();
        changes.mark_unchanged();

        engine
            .run_turn(
                crate::engine::runner::TurnRequest::new("hi"),
                std::sync::Arc::new(crate::engine::eval::presenter::NoopPresenter),
            )
            .await
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(2), changes.changed())
            .await
            .expect("probe should report the allocated window")
            .unwrap();
        assert_eq!(engine.context_limit(), Some(262_144));
        assert_eq!(engine.context_limit_source(), ContextLimitSource::Provider);
        assert_eq!(engine.context_probe_error(), None);
    }

    #[tokio::test]
    async fn probe_reports_failures_until_the_next_success() {
        let context = ContextTracker::new(None, catalog_with_trained_window());
        let host = serve_http(vec![
            ("500 Internal Server Error", "{}".to_string()),
            ("200 OK", ps_body(262_144)),
        ])
        .await;

        probe(host.clone(), &context).run().await;
        let error = context.probe_error().unwrap();
        assert!(error.contains("/api/ps returned HTTP 500"), "{error}");
        assert_eq!(context.limit_for(MODEL, "local"), Some(1_048_576));

        probe(host, &context).run().await;
        assert_eq!(context.probe_error(), None);
        assert_eq!(context.limit_for(MODEL, "local"), Some(262_144));
    }

    #[tokio::test]
    async fn probe_keeps_current_window_when_model_is_not_loaded() {
        let context = ContextTracker::new(None, catalog_with_trained_window());
        let host = serve_http(vec![("200 OK", r#"{"models":[]}"#.to_string())]).await;

        probe(host, &context).run().await;

        assert_eq!(context.limit_for(MODEL, "local"), Some(1_048_576));
        assert_eq!(context.probe_error(), None);
    }
}
