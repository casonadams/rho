pub use crate::repeat;
pub use crate::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook, normalized_call_key};
pub use tracking::{SessionUsageTotals, SpeedTracker};
pub mod builder;
pub use builder::{AgentEngineBuilder, create_engine_model};
pub mod compactor;
pub mod context;
pub mod metrics;
pub mod runner;
pub mod runtime;
pub mod tracking;

pub use compactor::CompactionStats;

#[cfg(test)]
mod tests;

pub mod eval;

use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionManager;
use rig::agent::Agent;
use std::str::FromStr;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

use metrics::format_tokens;

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub(crate) tools: Vec<rig::tool::DynamicTool>,
    pub(crate) tool_names: Arc<std::sync::RwLock<Vec<String>>>,
    pub(crate) plugins: Vec<Arc<dyn crate::plugin::RhoPlugin>>,
    pub(crate) agent: Arc<tokio::sync::RwLock<Agent>>,
    pub(crate) usage: UsageTracker,
    pub(crate) quota: QuotaTracker,
    pub(crate) context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
    pub(crate) project_context: Arc<tokio::sync::Mutex<Option<(std::path::PathBuf, context::ProjectContext)>>>,
    pub(crate) auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    pub(crate) model: Option<rig::agent::ModelHandle>,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        builder::AgentEngineBuilder::new(config, auth_store)
            .resume(resume_id)
            .build()
            .await
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tool_names.read().unwrap().clone()
    }

    pub fn shared_auth_store(&self) -> Arc<tokio::sync::Mutex<AuthStore>> {
        Arc::clone(&self.auth_store)
    }

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
        if let Ok(provider_id) = rho_harness_core::provider::ProviderId::from_str(self.config.provider.trim()) {
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

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        let base_dir = std::env::current_dir()?;
        let rebuilt = builder::AgentEngineBuilder::new(config, auth_store)
            .session(self.session_manager.clone())
            .base_dir(base_dir)
            .build()
            .await?;
        rebuilt.refresh_quota().await;
        Ok(rebuilt)
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model)
    }

    pub async fn project_context(&self) -> Result<context::ProjectContext> {
        let cwd = std::env::current_dir()?;
        let mut cache = self.project_context.lock().await;
        if cache.as_ref().map(|(dir, _)| dir.as_path()) != Some(cwd.as_path()) {
            *cache = Some((
                cwd.clone(),
                context::ProjectContext::discover_with_config(&cwd, &self.config).await,
            ));
        }
        let Some((_, cached)) = cache.as_mut() else {
            return Err(AppError::Other(anyhow::anyhow!("project context cache unavailable")));
        };
        cached.refresh_runtime_state().await;
        Ok(cached.clone())
    }

    pub async fn instruction_files(&self) -> Vec<String> {
        self.project_context()
            .await
            .map(|ctx| ctx.instruction_files.into_iter().map(|(path, _)| path).collect())
            .unwrap_or_default()
    }

    pub async fn activate_path_instructions(&self, path: &std::path::Path) {
        let mut cache = self.project_context.lock().await;
        if let Some((_, cached)) = cache.as_mut() {
            cached.activate_path_instructions_async(path).await;
        }
    }

    pub fn context_usage_percent(&self) -> Option<usize> {
        let usage = self.usage.latest()?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        Some(((consumed as usize * 100) / limit).min(100))
    }

    pub fn context_percent_f64(&self) -> Option<f64> {
        let usage = self.usage.latest()?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        Some(((consumed as f64 / limit as f64) * 100.0).clamp(0.0, 100.0))
    }

    pub fn session_usage_totals(&self) -> SessionUsageTotals {
        self.usage.totals()
    }

    pub fn usage(&self) -> &UsageTracker {
        &self.usage
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.usage.tokens_per_second()
    }

    pub fn context_display(&self) -> String {
        self.context_remaining_display()
    }

    pub fn context_remaining_display(&self) -> String {
        let limit = self.context_limit();
        let usage = self.usage.latest();
        match (usage, limit) {
            (Some(usage), Some(limit)) if usage.has_values() => {
                let consumed = usage.input_tokens
                    + usage.cached_input_tokens.unwrap_or(0)
                    + usage.cache_creation_input_tokens.unwrap_or(0);
                let remaining = limit.saturating_sub(consumed as usize);
                let percent = (remaining as f64 / limit as f64) * 100.0;
                let percent_str = if (percent.fract() * 10.0).round() == 0.0 {
                    format!("{percent:.0}%")
                } else {
                    format!("{percent:.1}%")
                };
                format!("{percent_str} ({})", format_tokens(limit as u64))
            }
            (None, Some(limit)) | (Some(_), Some(limit)) => format!("100% ({})", format_tokens(limit as u64)),
            (Some(usage), None) if usage.has_values() => format!("{} tokens", format_tokens(usage.input_tokens)),
            _ => "100%".to_string(),
        }
    }

    pub fn context_usage_display(&self) -> String {
        let Some(usage) = self.usage.latest() else {
            return "usage unavailable".to_string();
        };
        if !usage.has_values() {
            return "usage unavailable".to_string();
        }
        let consumed = usage.input_tokens
            + usage.cached_input_tokens.unwrap_or(0)
            + usage.cache_creation_input_tokens.unwrap_or(0);
        if let Some(limit) = self.context_limit() {
            let percent = ((consumed as usize * 100) / limit).min(100);
            format!(
                "{}/{} ({percent}%)",
                format_tokens(consumed),
                format_tokens(limit as u64)
            )
        } else {
            format!("{} input tokens", format_tokens(consumed))
        }
    }

    pub async fn refresh_quota(&self) {
        let provider = self.config.provider.trim();
        if provider.eq_ignore_ascii_case("ollama-cloud") {
            do_refresh_ollama_quota(Arc::clone(&self.auth_store), self.quota.clone()).await;
        } else if provider.eq_ignore_ascii_case("antigravity") || provider.eq_ignore_ascii_case("google-antigravity") {
            do_refresh_antigravity_quota(
                Arc::clone(&self.auth_store),
                self.quota.clone(),
                self.config.model.clone(),
            )
            .await;
        }
    }

    pub fn spawn_refresh_quota(&self) {
        let provider = self.config.provider.trim().to_string();
        if provider.eq_ignore_ascii_case("ollama-cloud") {
            let auth = Arc::clone(&self.auth_store);
            let quota = self.quota.clone();
            tokio::spawn(async move {
                do_refresh_ollama_quota(auth, quota).await;
            });
        } else if provider.eq_ignore_ascii_case("antigravity") || provider.eq_ignore_ascii_case("google-antigravity") {
            let auth = Arc::clone(&self.auth_store);
            let quota = self.quota.clone();
            let model = self.config.model.clone();
            tokio::spawn(async move {
                do_refresh_antigravity_quota(auth, quota, model).await;
            });
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.latest()
    }
}

async fn do_refresh_ollama_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker) {
    if !quota.should_fetch() {
        return;
    }
    let key = auth_store.lock().await.get_key("ollama-cloud").await.ok().flatten();
    let Some(key) = key else {
        quota.record_failure();
        return;
    };
    match crate::ollama::fetch_quota(&key).await {
        Some(display) => quota.record_success(display),
        None => quota.record_failure(),
    }
}

async fn do_refresh_antigravity_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    target_model: String,
) {
    if !quota.should_fetch() {
        return;
    }
    let (token, project_id) = {
        let mut store = auth_store.lock().await;
        let token = match store.get_key("antigravity").await {
            Ok(Some(t)) => t,
            _ => {
                quota.record_failure();
                return;
            }
        };
        let project_id = match store.get_credential("antigravity") {
            Some(rho_harness_core::auth::StoredCredential::OAuth {
                account_id: Some(id), ..
            }) => id.clone(),
            _ => crate::auth::antigravity::stable_project_id("antigravity-default"),
        };
        (token, project_id)
    };
    match crate::antigravity::fetch_quota(&token, &project_id, &target_model).await {
        Some(display) => quota.record_success(display),
        None => quota.record_failure(),
    }
}
