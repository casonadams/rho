use rho_harness_core::config::Config;
use rho_harness_core::error::Result;

use super::AgentEngine;
use super::tracking::{ContextLimitSource, ContextTracker};
use crate::provider::ModelCatalog;
use rho_harness_core::provider::{ModelRoute, ProviderId};

impl AgentEngine {
    pub async fn build_model_handle(&self, config: &Config) -> Result<crate::engine::compactor::llm::ModelHandle> {
        self.router.build(config, &config.route()).await
    }

    pub async fn switch_model(&mut self, model: &str, provider: &str) -> Result<()> {
        self.config.model = model.to_string();
        self.config.provider = provider.to_string();
        self.update_model().await
    }

    pub async fn update_model(&mut self) -> Result<()> {
        let route = self.config.route();
        self.config.set_route(route);
        let model_handle = self.build_model_handle(&self.config).await?;
        self.model = Some(model_handle);
        self.context = ContextTracker::new(self.config.context_limit);

        self.force_refresh_quota();
        Ok(())
    }

    pub fn model_catalog(&self) -> &ModelCatalog {
        self.router.catalog()
    }

    pub fn context_limit(&self) -> Option<usize> {
        Some(self.resolve_context_limit().0)
    }

    pub fn context_limit_source(&self) -> ContextLimitSource {
        self.resolve_context_limit().1
    }

    fn resolve_context_limit(&self) -> (usize, ContextLimitSource) {
        self.context.resolve_limit(self.model_catalog(), &self.config.route())
    }

    /// Why rho could not read the local Ollama server's allocated window after the last turn.
    pub fn context_probe_error(&self) -> Option<String> {
        self.context.probe_error()
    }

    /// Ollama only reports the window it allocated while the model is loaded, which is guaranteed
    /// right after a turn; the OpenAI-compatible API cannot request a window size.
    pub(crate) fn spawn_local_context_probe(&self) {
        let route = self.config.route();
        if self.model.is_none() || route.provider != ProviderId::Local.as_str() || self.config.context_limit.is_some() {
            return;
        }
        let probe = LocalContextProbe {
            host: self.ollama_host.clone(),
            route,
            catalog: self.model_catalog().clone(),
            context: self.context.clone(),
        };
        tokio::spawn(probe.run());
    }
}

struct LocalContextProbe {
    host: String,
    route: ModelRoute,
    catalog: ModelCatalog,
    context: ContextTracker,
}

impl LocalContextProbe {
    async fn run(self) {
        match crate::ollama::local::loaded_context_length(&self.host, &self.route.model).await {
            Ok(Some(tokens)) => {
                self.catalog
                    .record_observed_context(&self.route.provider, &self.route.model, tokens);
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

    fn probe(host: String, catalog: &ModelCatalog, context: &ContextTracker) -> LocalContextProbe {
        LocalContextProbe {
            host,
            route: ModelRoute::new("local", MODEL),
            catalog: catalog.clone(),
            context: context.clone(),
        }
    }

    #[tokio::test]
    async fn completed_local_turn_replaces_trained_window_with_allocated_one() {
        let dir = tempfile::tempdir().unwrap();
        let model = MockAdapter::new("inkling-2-mini", ProviderId::Local.as_str());
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
        engine.router = crate::provider::ModelRouter::new(engine.shared_auth_store(), catalog.clone());
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

    fn window(catalog: &ModelCatalog) -> Option<usize> {
        catalog.context_tokens("local", MODEL)
    }

    #[tokio::test]
    async fn switching_to_an_aliased_prefixed_model_builds_its_canonical_route() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = crate::engine::eval::mock::mock_engine(
            std::sync::Arc::new(MockAdapter::text("unused")),
            crate::engine::eval::mock::MockEngineConfig {
                base_dir: dir.path(),
                app_config: Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );

        engine.switch_model("local/qwen2.5-coder:7b", "ollama").await.unwrap();

        assert_eq!(engine.config.canonical_model_spec(), "local/qwen2.5-coder:7b");
        let model = engine.model.as_ref().unwrap();
        assert_eq!(
            (model.provider_name(), model.model_name()),
            ("local", "qwen2.5-coder:7b")
        );
    }

    #[tokio::test]
    async fn probe_reports_failures_until_the_next_success() {
        let (catalog, context) = (catalog_with_trained_window(), ContextTracker::new(None));
        let host = serve_http(vec![
            ("500 Internal Server Error", "{}".to_string()),
            ("200 OK", ps_body(262_144)),
        ])
        .await;

        probe(host.clone(), &catalog, &context).run().await;
        let error = context.probe_error().unwrap();
        assert!(error.contains("/api/ps returned HTTP 500"), "{error}");
        assert_eq!(window(&catalog), Some(1_048_576));

        probe(host, &catalog, &context).run().await;
        assert_eq!(context.probe_error(), None);
        assert_eq!(window(&catalog), Some(262_144));
    }

    #[tokio::test]
    async fn probe_keeps_current_window_when_model_is_not_loaded() {
        let (catalog, context) = (catalog_with_trained_window(), ContextTracker::new(None));
        let host = serve_http(vec![("200 OK", r#"{"models":[]}"#.to_string())]).await;

        probe(host, &catalog, &context).run().await;

        assert_eq!(window(&catalog), Some(1_048_576));
        assert_eq!(context.probe_error(), None);
    }
}
