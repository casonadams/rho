pub mod builder;
pub mod context;
pub mod metrics;
pub mod provider;
pub mod quota;
pub mod runner;
pub mod runtime;
mod tracking;

#[cfg(test)]
mod eval;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::plugin::contract::{ProviderCapability, ScopedCredential};
use crate::plugin::tool_dispatch::ActiveToolSet;
use crate::plugin::{ExtensionContext, ExtensionRegistry};
use crate::session::SessionManager;
use rig::agent::Agent;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

use metrics::format_tokens;

pub(crate) enum AgentBackend {
    Rig(Box<Agent>),
    External {
        provider: Arc<dyn ProviderCapability>,
        tools: Arc<ActiveToolSet>,
        credential: Option<ScopedCredential>,
    },
}

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub extension_registry: ExtensionRegistry,
    pub(crate) backend: AgentBackend,
    usage: UsageTracker,
    quota: QuotaTracker,
    context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
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
            .base_dir(std::env::current_dir()?)
            .build()
            .await
    }

    pub fn extension_context(&self) -> ExtensionContext {
        let cwd = std::env::current_dir().unwrap_or_default();
        ExtensionContext::new(cwd, &self.session_manager.session_id)
            .with_model_info(&self.config.model, &self.config.provider)
    }

    pub fn context_limit(&self) -> Option<usize> {
        self.context.limit_for(&self.config.model)
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
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.latest()
    }

    pub(crate) fn record_usage(&self, usage: metrics::StructuralUsage) {
        self.usage.record(usage);
    }
}
