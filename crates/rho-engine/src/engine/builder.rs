use super::AgentEngine;
use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionManager;
use rig::agent::ModelHandle;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    base_dir: Option<PathBuf>,
    rig_tools: Option<Vec<rig::tool::DynamicTool>>,
    extra_tools: Vec<rig::tool::DynamicTool>,
    plugins: Vec<Arc<dyn crate::plugin::RhoPlugin>>,
    model: Option<ModelHandle>,
}

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
            rig_tools: None,
            extra_tools: Vec::new(),
            plugins: Vec::new(),
            config,
            auth_store,
            resume_id: None,
            session_manager: None,
            base_dir: None,
            model: None,
        }
    }

    pub fn model(mut self, model: ModelHandle) -> Self {
        self.model = Some(model);
        self
    }

    pub fn resume(mut self, resume_id: Option<&str>) -> Self {
        self.resume_id = resume_id.map(str::to_owned);
        self
    }

    pub fn tools(mut self, rig_tools: Vec<rig::tool::DynamicTool>) -> Self {
        self.rig_tools = Some(rig_tools);
        self
    }

    pub fn add_tool(mut self, tool: rig::tool::DynamicTool) -> Self {
        self.extra_tools.push(tool);
        self
    }

    pub fn add_tools(mut self, tools: impl IntoIterator<Item = rig::tool::DynamicTool>) -> Self {
        self.extra_tools.extend(tools);
        self
    }

    pub fn plugin(mut self, plugin: Arc<dyn crate::plugin::RhoPlugin>) -> Self {
        self.extra_tools.extend(plugin.tools());
        self.plugins.push(plugin);
        self
    }

    pub fn plugins(mut self, plugins: impl IntoIterator<Item = Arc<dyn crate::plugin::RhoPlugin>>) -> Self {
        for p in plugins {
            self = self.plugin(p);
        }
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
            None => {
                SessionManager::new_with_secrets_async(
                    &self.config.sessions_dir,
                    self.resume_id.as_deref(),
                    self.auth_store.secret_values(),
                )
                .await?
            }
        };

        let mut config = self.config;
        let mut auth_store = self.auth_store;
        let is_unmodified_default = config.provider == "local" && config.model == "llama3.2";

        // Auto-refresh expired OAuth tokens before building the model client
        // (get_key refreshes + persists when the stored token is stale).
        if let Ok(provider_id) = ProviderId::from_str(config.provider.trim()) {
            let _ = auth_store.get_key(provider_id.as_str()).await?;
        }

        let shared_auth = Arc::new(tokio::sync::Mutex::new(auth_store.clone()));

        let model = if let Some(m) = self.model {
            m
        } else {
            match create_engine_model(&config, &auth_store, Some(shared_auth.clone())) {
                Ok(m) => m,
                Err(e) => {
                    if is_unmodified_default {
                        let configured = auth_store.list_configured_providers();
                        let mut fallback = None;
                        for p in configured {
                            let default_model = default_model_for_provider(&p);
                            let mut trial_config = config.clone();
                            trial_config.provider = p.clone();
                            trial_config.model = default_model.to_string();
                            if let Ok(m) = create_engine_model(&trial_config, &auth_store, Some(shared_auth.clone())) {
                                config = trial_config;
                                fallback = Some(m);
                                break;
                            }
                        }

                        if let Some(m) = fallback {
                            m
                        } else if let Ok(local_model) = create_engine_model(
                            &Config {
                                provider: "local".to_string(),
                                model: "llama3.2".to_string(),
                                ..config.clone()
                            },
                            &auth_store,
                            Some(shared_auth.clone()),
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
            }
        };

        let context_limit = match config.context_limit {
            Some(limit) => Some(limit),
            None if matches!(config.provider.as_str(), "local" | "ollama" | "ollama-cloud") => {
                let store = crate::provider::ModelStore::load(config.config_dir.join("models-store.json"));
                let keys: &[&str] = if config.provider == "ollama-cloud" {
                    &["ollama-cloud"]
                } else {
                    &["local", "ollama"]
                };
                store.context_tokens(keys, &config.model)
            }
            None => None,
        };

        let custom_tools = self.rig_tools.is_some();
        let mut tools = match self.rig_tools {
            Some(t) => t,
            None => crate::tools::builtin_tools::build_builtin_tools(&base_dir, &config)?,
        };
        tools.extend(self.extra_tools);
        let tool_names = tools.iter().map(|t| t.name().to_string()).collect();

        let mcp_loader = if !custom_tools && config.mcp.enabled && !config.mcp.servers.is_empty() {
            let mcp_config = config.clone();
            let mcp_dir = base_dir.clone();
            let handle = tokio::spawn(async move { crate::mcp::load_mcp_tools(&mcp_config, &mcp_dir).await });
            Some(handle)
        } else {
            None
        };

        let agent = super::runtime::build_coding_agent(
            model.clone(),
            &config,
            CodingRuntime {
                base_dir: &base_dir,
                memory: session_manager.clone(),
                built_in_tools: Some(tools.clone()),
            },
        )?;

        Ok(AgentEngine {
            config: config.clone(),
            session_manager,
            tool_names: Arc::new(std::sync::RwLock::new(tool_names)),
            plugins: self.plugins,
            agent: Arc::new(tokio::sync::RwLock::new(agent)),
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(context_limit),
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
            auth_store: shared_auth,
            base_tools: tools,
            base_dir,
            model: Some(model),
            mcp_loader: Arc::new(tokio::sync::Mutex::new(mcp_loader)),
        })
    }
}

pub fn create_engine_model(
    config: &Config,
    auth_store: &AuthStore,
    shared_auth: Option<Arc<tokio::sync::Mutex<AuthStore>>>,
) -> Result<ModelHandle> {
    let name = config.provider.trim();
    if let Ok(provider_id) = ProviderId::from_str(name) {
        return crate::provider::ProviderFactory::create_model_for(
            crate::provider::ModelRequest {
                provider: provider_id,
                model: &config.model,
                thinking_level: config.thinking_level.as_deref(),
                shared_auth,
            },
            auth_store,
        );
    }
    crate::provider::ProviderFactory::create_model(config, &config.model, auth_store)
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
