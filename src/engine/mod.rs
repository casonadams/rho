pub mod context;
pub mod metrics;
pub mod provider;
pub mod quota;
pub mod runner;
pub mod runtime;

#[cfg(test)]
mod eval;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::provider::{CredentialStrategy, ModelRequest, ProviderFactory, ProviderId};
use crate::error::Result;
use crate::plugin::{ExtensionContext, ExtensionRegistry, PluginLoader};
use crate::session::SessionManager;
use rig::agent::Agent;
use rig::completion::Usage;
use runtime::CodingRuntime;
use std::sync::Mutex;

use metrics::{RunTracker, format_tokens};

pub struct AgentEngine {
    pub config: Config,
    pub session_manager: SessionManager,
    pub extension_registry: ExtensionRegistry,
    agent: Agent,
    last_usage: Mutex<Option<Usage>>,
    last_quota: Mutex<Option<String>>,
    run_tracker: RunTracker,
}

impl AgentEngine {
    pub async fn new(config: Config, auth_store: AuthStore, resume_id: Option<&str>) -> Result<Self> {
        let session_manager =
            SessionManager::new_with_secrets(&config.sessions_dir, resume_id, auth_store.secret_values())?;
        Self::with_session(config, auth_store, session_manager)
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        let mut engine = Self::with_session(config, auth_store, self.session_manager.clone())?;
        engine.extension_registry = self.extension_registry.clone();
        Ok(engine)
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

        let mut extension_registry = ExtensionRegistry::new();
        if let Ok(discovery) = PluginLoader::discover(&config.config_dir, Some(&base_dir)) {
            let _ = PluginLoader::load_discovered(&discovery, &mut extension_registry);
        }

        Ok(Self {
            config,
            session_manager,
            extension_registry,
            agent,
            last_usage: Mutex::new(None),
            last_quota: Mutex::new(None),
            run_tracker: RunTracker::default(),
        })
    }

    pub fn extension_context(&self) -> ExtensionContext {
        let cwd = std::env::current_dir().unwrap_or_default();
        ExtensionContext::new(cwd, &self.session_manager.session_id)
            .with_model_info(&self.config.model, &self.config.provider)
    }

    pub fn context_limit(&self) -> Option<usize> {
        if let Some(limit) = self.config.context_limit {
            return Some(limit);
        }
        let lower = self.config.model.to_ascii_lowercase();
        if lower.contains("gpt-5") || lower.contains("luna") || lower.contains("codex") {
            Some(376_000)
        } else if lower.contains("o1") || lower.contains("o3") {
            Some(200_000)
        } else if lower.contains("gpt-4") {
            Some(128_000)
        } else if lower.contains("claude") || lower.contains("gemini") {
            Some(1_000_000)
        } else if lower.contains("deepseek") {
            Some(128_000)
        } else {
            None
        }
    }

    pub fn context_usage_percent(&self) -> Option<usize> {
        let usage = self.last_usage.lock().ok().and_then(|usage| *usage)?;
        if !usage.has_values() {
            return None;
        }
        let limit = self.context_limit()?;
        Some(((usage.input_tokens as usize * 100) / limit).min(100))
    }

    pub fn context_display(&self) -> String {
        let limit = self.context_limit();
        let usage = self.last_usage.lock().ok().and_then(|usage| *usage);
        match (usage, limit) {
            (Some(usage), Some(limit)) if usage.has_values() => {
                let percent = (usage.input_tokens as f64 / limit as f64) * 100.0;
                let percent_str = if percent < 0.05 && usage.input_tokens > 0 {
                    "0.1%".to_string()
                } else if (percent.fract() * 10.0).round() == 0.0 {
                    format!("{:.0}%", percent)
                } else {
                    format!("{:.1}%", percent)
                };
                format!("{percent_str} ({})", format_tokens(limit as u64))
            }
            (None, Some(limit)) | (Some(_), Some(limit)) => {
                format!("0% ({})", format_tokens(limit as u64))
            }
            (Some(usage), None) if usage.has_values() => {
                format!("{} tokens", format_tokens(usage.input_tokens))
            }
            _ => "0%".to_string(),
        }
    }

    pub fn context_remaining_display(&self) -> String {
        self.context_display()
    }

    pub fn context_usage_display(&self) -> String {
        let usage = self.last_usage.lock().ok().and_then(|usage| *usage);
        let Some(usage) = usage else {
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
            && let Ok(mut lock) = self.last_quota.lock()
        {
            *lock = Some(formatted);
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.last_quota.lock().ok().and_then(|lock| lock.clone())
    }

    fn record_usage(&self, usage: Usage) {
        if let Ok(mut current) = self.last_usage.lock() {
            *current = usage.has_values().then_some(usage);
        }
    }
}
