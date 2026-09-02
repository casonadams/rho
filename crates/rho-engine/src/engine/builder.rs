use super::AgentEngine;
use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use rho_core::config::Config;
use rho_core::error::Result;
use rho_core::session::SessionManager;
use std::path::PathBuf;
use std::sync::Arc;

pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    base_dir: Option<PathBuf>,
    rig_tools: Option<Vec<rig::tool::DynamicTool>>,
}

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
            rig_tools: None,
            config,
            auth_store,
            resume_id: None,
            session_manager: None,
            base_dir: None,
        }
    }

    pub fn resume(mut self, resume_id: Option<&str>) -> Self {
        self.resume_id = resume_id.map(str::to_owned);
        self
    }

    pub fn tools(mut self, rig_tools: Vec<rig::tool::DynamicTool>) -> Self {
        self.rig_tools = Some(rig_tools);
        self
    }

    pub fn session(mut self, session_manager: SessionManager) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    pub fn base_dir(mut self, base_dir: PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    pub async fn build(self) -> Result<AgentEngine> {
        let base_dir = self.base_dir.unwrap_or(std::env::current_dir()?);
        let session_manager = match self.session_manager {
            Some(session) => session,
            None => SessionManager::new_with_secrets(
                &self.config.sessions_dir,
                self.resume_id.as_deref(),
                self.auth_store.secret_values(),
            )?,
        };

        let mut config = self.config;
        let is_unmodified_default = config.provider == "anthropic" && config.model == "claude-3-7-sonnet-20250219";

        let model = match crate::provider::ProviderFactory::create_model(&config, &config.model, &self.auth_store) {
            Ok(m) => m,
            Err(e) => {
                // ONLY auto-fallback if we are on the untouched default startup config
                // and Anthropic credentials don't exist.
                // If the user selected ANY explicit model or provider, return the error directly!
                if is_unmodified_default {
                    let configured = self.auth_store.list_configured_providers();
                    let mut fallback = None;
                    for p in configured {
                        let default_model = default_model_for_provider(&p);
                        let mut trial_config = config.clone();
                        trial_config.provider = p.clone();
                        trial_config.model = default_model.to_string();
                        if let Ok(m) = crate::provider::ProviderFactory::create_model(
                            &trial_config,
                            &trial_config.model,
                            &self.auth_store,
                        ) {
                            config = trial_config;
                            fallback = Some(m);
                            break;
                        }
                    }

                    if let Some(m) = fallback {
                        m
                    } else if let Ok(local_model) = crate::provider::ProviderFactory::create_model(
                        &Config {
                            provider: "local".to_string(),
                            model: "llama3.2".to_string(),
                            ..config.clone()
                        },
                        "llama3.2",
                        &self.auth_store,
                    ) {
                        config.provider = "local".to_string();
                        config.model = "llama3.2".to_string();
                        local_model
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            }
        };

        let context_limit = config.context_limit;
        let agent = super::runtime::build_coding_agent(
            model,
            &config,
            CodingRuntime {
                base_dir: &base_dir,
                memory: session_manager.clone(),
                built_in_tools: self.rig_tools.clone(),
            },
        )?;

        Ok(AgentEngine {
            config: config.clone(),
            session_manager,
            tool_names: self
                .rig_tools
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|tool| tool.name().to_string())
                .collect(),
            agent: Box::new(agent),
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(context_limit),
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
        })
    }
}

fn default_model_for_provider(provider: &str) -> &'static str {
    match provider.to_ascii_lowercase().as_str() {
        "chatgpt" => "gpt-5.4",
        "openai" | "copilot" => "gpt-4o",
        "gemini" => "gemini-2.0-flash",
        "deepseek" => "deepseek-chat",
        "groq" => "llama-3.3-70b-versatile",
        "openrouter" => "anthropic/claude-3.7-sonnet",
        "xai" => "grok-2-latest",
        "mistral" => "mistral-large-latest",
        "cohere" => "command-r-plus",
        "ollama" | "local" => "llama3.2",
        "ollama-cloud" => "glm-5.3-flash",
        _ => "claude-3-7-sonnet-20250219",
    }
}
