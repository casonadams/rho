pub use crate::repeat;
pub use crate::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook, normalized_call_key};
pub use rho_plugin_providers::quota;
pub mod builder;
pub mod context;
pub mod metrics;
pub mod provider;
pub mod runner;
pub mod runtime;
pub mod tracking;

pub mod eval;

use crate::auth::AuthStore;
use rho_core::config::Config;
use rho_core::dispatch::NeutralToolExecutor;
use rho_core::error::{AppError, Result};
use rho_core::presentation::Presenter;
use rho_core::session::SessionManager;
use rho_sdk::contract::{
    ContextCapability, ContextRequest, InvocationContext, LifecycleCapability, LifecycleEvent, ProviderCapability,
    ScopedCredential,
};
use rig::agent::Agent;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

use metrics::format_tokens;

pub enum AgentBackend {
    Rig(Box<Agent>),
    External {
        provider: Arc<dyn ProviderCapability>,
        /// The host platform's active tool set, exposed to the engine only
        /// through the neutral executor contract.
        tools: std::sync::Arc<dyn NeutralToolExecutor>,
        credential: Option<ScopedCredential>,
    },
}

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub session_approvals: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) backend: AgentBackend,
    pub(crate) contexts: Vec<Arc<dyn ContextCapability>>,
    pub(crate) lifecycles: Vec<Arc<dyn LifecycleCapability>>,
    pub(crate) usage: UsageTracker,
    pub(crate) quota: QuotaTracker,
    pub(crate) context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
    pub(crate) project_context: Arc<tokio::sync::Mutex<Option<(std::path::PathBuf, context::ProjectContext)>>>,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        builder::AgentEngineBuilder::new(config, auth_store)
            .resume(resume_id)
            .build()
            .await
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        builder::AgentEngineBuilder::new(config, auth_store)
            .session(self.session_manager.clone())
            .session_approvals(self.session_approvals.clone())
            .contexts(self.contexts.clone())
            .lifecycles(self.lifecycles.clone())
            .base_dir(std::env::current_dir()?)
            .build()
            .await
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model)
    }

    /// Discover project context once per working directory and reuse it for
    /// subsequent turns of the session; only the wall-clock-dependent parts
    /// (git status, current date) are refreshed per turn.
    pub(crate) async fn project_context(&self) -> Result<context::ProjectContext> {
        let cwd = std::env::current_dir()?;
        let mut cache = self.project_context.lock().await;
        if cache.as_ref().map(|(dir, _)| dir.as_path()) != Some(cwd.as_path()) {
            *cache = Some((
                cwd.clone(),
                context::ProjectContext::discover(&cwd, Some(&self.config.config_dir)).await,
            ));
        }
        let Some((_, cached)) = cache.as_mut() else {
            return Err(AppError::Other(anyhow::anyhow!("project context cache unavailable")));
        };
        cached.refresh_runtime_state().await;
        Ok(cached.clone())
    }

    pub fn context_usage_percent(&self) -> Option<usize> {
        let usage = self.usage.latest()?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        Some(((usage.input_tokens as usize * 100) / limit).min(100))
    }

    pub fn context_display(&self) -> String {
        let limit = self.context_limit();
        let usage = self.usage.latest();
        match (usage, limit) {
            (Some(usage), Some(limit)) if usage.has_values() => {
                let percent = (usage.input_tokens as f64 / limit as f64) * 100.0;
                let percent_str = if percent < 0.05 && usage.input_tokens > 0 {
                    "0.1%".to_string()
                } else if (percent.fract() * 10.0).round() == 0.0 {
                    format!("{percent:.0}%")
                } else {
                    format!("{percent:.1}%")
                };
                format!("{percent_str} ({})", format_tokens(limit as u64))
            }
            (None, Some(limit)) | (Some(_), Some(limit)) => format!("0% ({})", format_tokens(limit as u64)),
            (Some(usage), None) if usage.has_values() => format!("{} tokens", format_tokens(usage.input_tokens)),
            _ => "0%".to_string(),
        }
    }

    pub fn context_remaining_display(&self) -> String {
        self.context_display()
    }

    pub fn context_usage_display(&self) -> String {
        let Some(usage) = self.usage.latest() else {
            return "usage unavailable".to_string();
        };
        if !usage.has_values() {
            return "usage unavailable".to_string();
        }
        if let Some(limit) = self.context_limit() {
            let percent = ((usage.input_tokens as usize * 100) / limit).min(100);
            format!(
                "{}/{} ({percent}%)",
                format_tokens(usage.input_tokens),
                format_tokens(limit as u64)
            )
        } else {
            format!("{} input tokens", format_tokens(usage.input_tokens))
        }
    }

    pub async fn refresh_quota(&self) {
        if self.config.provider == "chatgpt"
            && let Some(formatted) = crate::engine::quota::fetch_chatgpt_quota(&self.config.config_dir).await
        {
            self.quota.replace(Some(formatted));
        } else if self.config.provider == "ollama"
            && crate::engine::quota::is_ollama_cloud_model(&self.config.model)
            && let Some(formatted) =
                crate::engine::quota::fetch_ollama_cloud_quota(&self.config.config_dir, &self.config.model).await
        {
            self.quota.replace(Some(formatted));
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.latest()
    }

    pub(crate) fn record_usage(&self, usage: metrics::StructuralUsage) {
        self.usage.record(usage);
    }

    pub(crate) async fn augment_prompt_with_context(&self, prompt: &str, presenter: &Arc<dyn Presenter>) -> String {
        if self.contexts.is_empty() {
            return prompt.to_string();
        }
        let session_id = self.session_manager.session_id.clone();
        let working_directory = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let has_interactive_ui = presenter.has_interactive_ui();
        let token_budget = self.config.context_injection_max_tokens;

        let mut futures = Vec::new();
        for ctx_cap in &self.contexts {
            let cap = Arc::clone(ctx_cap);
            let req = ContextRequest {
                prompt: prompt.to_string(),
                context: InvocationContext {
                    session_id: session_id.clone(),
                    working_directory: working_directory.clone(),
                    has_interactive_ui,
                },
                token_budget: Some(token_budget),
            };
            futures.push(async move {
                match tokio::time::timeout(std::time::Duration::from_millis(2000), cap.retrieve(req)).await {
                    Ok(Ok(response)) => Ok(response.snippets),
                    Ok(Err(e)) => Err(format!("Context retrieval failed: {e}")),
                    Err(_) => Err("Context retrieval timed out after 2000ms".to_string()),
                }
            });
        }

        let results = futures::future::join_all(futures).await;
        let mut all_snippets = Vec::new();
        for res in results {
            match res {
                Ok(snippets) => all_snippets.extend(snippets),
                Err(err) => {
                    presenter.print_notice(&format!("  [{err}]\n"));
                }
            }
        }

        if all_snippets.is_empty() {
            return prompt.to_string();
        }

        all_snippets.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut context_block = String::from("<retrieved_context>\n");
        let mut total_chars = 0;
        let max_chars = token_budget * 4;

        for snippet in all_snippets {
            let snippet_text = if let Some(title) = &snippet.title {
                format!(
                    "<document source=\"{}\" title=\"{}\">\n{}\n</document>\n",
                    snippet.source, title, snippet.content
                )
            } else {
                format!(
                    "<document source=\"{}\">\n{}\n</document>\n",
                    snippet.source, snippet.content
                )
            };
            if total_chars + snippet_text.len() > max_chars && total_chars > 0 {
                break;
            }
            total_chars += snippet_text.len();
            context_block.push_str(&snippet_text);
        }
        context_block.push_str("</retrieved_context>\n\n");
        context_block.push_str(prompt);
        context_block
    }

    pub(crate) async fn notify_before_turn(&self, prompt: &str) {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        for lifecycle in &self.lifecycles {
            let _ = lifecycle
                .notify(LifecycleEvent::BeforeTurn {
                    session_id: self.session_manager.session_id.clone(),
                    prompt: prompt.to_string(),
                    working_directory: cwd.clone(),
                })
                .await;
        }
    }

    pub(crate) async fn notify_after_turn(&self, success: bool) {
        for lifecycle in &self.lifecycles {
            let _ = lifecycle
                .notify(LifecycleEvent::AfterTurn {
                    session_id: self.session_manager.session_id.clone(),
                    success,
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rho_sdk::capability::CapabilityError;
    use rho_sdk::contract::{ContextDescriptor, ContextResponse, ContextSnippet};
    use rig::test_utils::MockCompletionModel;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct TestContextCap {
        snippets: Vec<ContextSnippet>,
        should_fail: bool,
    }

    #[async_trait]
    impl ContextCapability for TestContextCap {
        fn descriptor(&self) -> ContextDescriptor {
            ContextDescriptor {
                id: "context:test".parse().unwrap(),
                display_name: "Test Context".to_string(),
                description: "Test".to_string(),
                max_snippets: Some(5),
            }
        }

        async fn retrieve(&self, _request: ContextRequest) -> std::result::Result<ContextResponse, CapabilityError> {
            if self.should_fail {
                Err(CapabilityError::Failed {
                    message: "database locked".to_string(),
                })
            } else {
                Ok(ContextResponse {
                    snippets: self.snippets.clone(),
                })
            }
        }
    }

    struct TestLifecycleCap {
        before_called: Arc<AtomicBool>,
        after_called: Arc<AtomicBool>,
    }

    #[async_trait]
    impl LifecycleCapability for TestLifecycleCap {
        fn id(&self) -> rho_sdk::capability::CapabilityId {
            "lifecycle:test".parse().unwrap()
        }

        async fn notify(&self, event: LifecycleEvent) -> std::result::Result<(), CapabilityError> {
            match event {
                LifecycleEvent::BeforeTurn { prompt, .. } if prompt == "hello" => {
                    self.before_called.store(true, Ordering::Relaxed);
                }
                LifecycleEvent::AfterTurn { success, .. } if success => {
                    self.after_called.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn context_augmentation_formats_snippets_and_caps_tokens() {
        let temp_dir = std::env::temp_dir().join(format!("ctx_aug_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let ctx_cap = Arc::new(TestContextCap {
            snippets: vec![
                ContextSnippet {
                    source: "doc1.md".to_string(),
                    title: Some("Doc 1".to_string()),
                    content: "Content 1".to_string(),
                    score: Some(0.8),
                },
                ContextSnippet {
                    source: "doc2.md".to_string(),
                    title: None,
                    content: "Content 2".to_string(),
                    score: Some(0.95),
                },
            ],
            should_fail: false,
        });

        let config = Config {
            context_injection_max_tokens: 1000,
            ..Config::default()
        };

        let mut engine = eval::mock::mock_engine(
            MockCompletionModel::new(vec![]),
            eval::mock::MockEngineConfig {
                base_dir: &temp_dir,
                app_config: config,
                session_manager: None,
                built_in_tools: None,
            },
        );
        engine.contexts = vec![ctx_cap];

        let presenter = eval::presenter::presenter();
        let augmented = engine.augment_prompt_with_context("what is doc?", &presenter).await;
        assert!(augmented.starts_with("<retrieved_context>\n"));
        assert!(augmented.contains("<document source=\"doc2.md\">\nContent 2\n</document>"));
        assert!(augmented.contains("<document source=\"doc1.md\" title=\"Doc 1\">\nContent 1\n</document>"));
        assert!(augmented.ends_with("what is doc?"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn context_retrieval_failure_proceeds_with_unaugmented_prompt() {
        let temp_dir = std::env::temp_dir().join(format!("ctx_fail_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let ctx_cap = Arc::new(TestContextCap {
            snippets: Vec::new(),
            should_fail: true,
        });

        let mut engine = eval::mock::mock_engine(
            MockCompletionModel::new(vec![]),
            eval::mock::MockEngineConfig {
                base_dir: &temp_dir,
                app_config: Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        engine.contexts = vec![ctx_cap];

        let presenter = eval::presenter::presenter();
        let augmented = engine.augment_prompt_with_context("plain prompt", &presenter).await;
        assert_eq!(augmented, "plain prompt");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn lifecycle_notifications_reach_active_listeners() {
        let temp_dir = std::env::temp_dir().join(format!("lifecycle_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let before_called = Arc::new(AtomicBool::new(false));
        let after_called = Arc::new(AtomicBool::new(false));
        let lifecycle_cap = Arc::new(TestLifecycleCap {
            before_called: Arc::clone(&before_called),
            after_called: Arc::clone(&after_called),
        });

        let mut engine = eval::mock::mock_engine(
            MockCompletionModel::new(vec![]),
            eval::mock::MockEngineConfig {
                base_dir: &temp_dir,
                app_config: Config::default(),
                session_manager: None,
                built_in_tools: None,
            },
        );
        engine.lifecycles = vec![lifecycle_cap];

        engine.notify_before_turn("hello").await;
        assert!(before_called.load(Ordering::Relaxed));

        engine.notify_after_turn(true).await;
        assert!(after_called.load(Ordering::Relaxed));

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
