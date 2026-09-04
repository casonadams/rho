pub use crate::repeat;
pub use crate::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook, normalized_call_key};
pub use tracking::{SessionUsageTotals, SpeedTracker};
pub mod builder;
pub mod context;
pub mod metrics;
pub mod runner;
pub mod runtime;
pub mod tracking;

#[cfg(test)]
mod tests;

pub mod eval;

use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionManager;
use rig::agent::Agent;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

use metrics::format_tokens;

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub tool_names: Vec<String>,
    pub(crate) plugins: Vec<Arc<dyn crate::plugin::RhoPlugin>>,
    pub(crate) agent: Box<Agent>,
    pub(crate) usage: UsageTracker,
    pub(crate) quota: QuotaTracker,
    pub(crate) context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
    pub(crate) project_context: Arc<tokio::sync::Mutex<Option<(std::path::PathBuf, context::ProjectContext)>>>,
    pub(crate) auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        builder::AgentEngineBuilder::new(config, auth_store)
            .resume(resume_id)
            .build()
            .await
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        let base_dir = std::env::current_dir()?;
        let mut tools = crate::tools::build_builtin_tools(&base_dir, &config)?;
        tools.extend(crate::mcp::load_mcp_tools(&config, &base_dir).await);
        builder::AgentEngineBuilder::new(config, auth_store)
            .session(self.session_manager.clone())
            .base_dir(base_dir)
            .tools(tools)
            .build()
            .await
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model)
    }

    pub(crate) async fn project_context(&self) -> Result<context::ProjectContext> {
        let cwd = std::env::current_dir()?;
        let mut cache = self.project_context.lock().await;
        if cache.as_ref().map(|(dir, _)| dir.as_path()) != Some(cwd.as_path()) {
            *cache = Some((
                cwd.clone(),
                context::ProjectContext::discover(
                    &cwd,
                    Some(&self.config.config_dir),
                    !self.config.disable_built_in_skills,
                )
                .await,
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
        if !provider.eq_ignore_ascii_case("antigravity") && !provider.eq_ignore_ascii_case("google-antigravity") {
            return;
        }
        if !self.quota.should_fetch() {
            return;
        }
        let (token, project_id) = {
            let mut store = self.auth_store.lock().await;
            let token = match store.get_key("antigravity").await {
                Ok(Some(t)) => t,
                _ => {
                    self.quota.record_failure();
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
        match crate::antigravity::fetch_quota(&token, &project_id, &self.config.model).await {
            Some(display) => self.quota.record_success(display),
            None => self.quota.record_failure(),
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.latest()
    }
}
