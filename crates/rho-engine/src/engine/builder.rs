use super::AgentEngine;
use super::runtime::CodingRuntime;
use super::tracking::{ContextTracker, QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionManager;
use rig::agent::ModelHandle;
use rig::tool::DynamicTool;
use std::path::{Path, PathBuf};
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
}

fn try_provider_default(
    config: &mut Config,
    auth_store: &AuthStore,
    shared_auth: &Arc<tokio::sync::Mutex<AuthStore>>,
) -> Option<ModelHandle> {
    let default_model = default_model_for_provider(&config.provider);
    let mut trial = config.clone();
    trial.model = default_model.to_string();
    let m = create_engine_model(&trial, auth_store, Some(shared_auth.clone())).ok()?;
    *config = trial;
    Some(m)
}

fn try_configured_providers(
    config: &mut Config,
    auth_store: &AuthStore,
    shared_auth: &Arc<tokio::sync::Mutex<AuthStore>>,
) -> Option<ModelHandle> {
    for p in auth_store.list_configured_providers() {
        let default_model = default_model_for_provider(&p);
        let mut trial = config.clone();
        trial.provider = p;
        trial.model = default_model.to_string();
        if let Ok(m) = create_engine_model(&trial, auth_store, Some(shared_auth.clone())) {
            *config = trial;
            return Some(m);
        }
    }
    None
}

fn resolve_model_or_fallback(
    (config, auth_store, shared_auth): (&mut Config, &AuthStore, Arc<tokio::sync::Mutex<AuthStore>>),
) -> Result<ModelHandle> {
    if let Ok(m) = create_engine_model(config, auth_store, Some(shared_auth.clone())) {
        return Ok(m);
    }
    let is_provider_without_model =
        config.default_model.is_none() && config.provider != "local" && config.model == "llama3.2";
    if is_provider_without_model {
        if let Some(m) = try_provider_default(config, auth_store, &shared_auth) {
            return Ok(m);
        }
    } else if config.default_model.is_none()
        && config.provider == "local"
        && config.model == "llama3.2"
        && let Some(m) = try_configured_providers(config, auth_store, &shared_auth)
    {
        return Ok(m);
    }
    create_engine_model(config, auth_store, Some(shared_auth))
}

async fn build_engine_tools(
    (base_dir, config): (&Path, &Config),
    rig_tools: Option<Vec<DynamicTool>>,
    extra_tools: Vec<DynamicTool>,
) -> Result<Vec<DynamicTool>> {
    let mut tools = match rig_tools {
        Some(t) => t,
        None => {
            let mut t = crate::tools::builtin_tools::build_builtin_tools(base_dir, config)?;
            if config.mcp.enabled && !config.mcp.servers.is_empty() {
                t.extend(crate::mcp::load_mcp_tools(config, base_dir).await);
            }
            t
        }
    };
    tools.extend(extra_tools);
    Ok(tools)
}

impl AgentEngineBuilder {
    async fn resolve_session(&self) -> Result<SessionManager> {
        let session_manager = match self.session_manager.clone() {
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
        if let Some(days) = self.config.session_retention_days
            && days > 0
        {
            session_manager.spawn_auto_prune(days);
        }
        Ok(session_manager)
    }

    async fn validate_provider_auth(&mut self) -> Result<()> {
        if let Ok(provider_id) = ProviderId::from_str(self.config.provider.trim()) {
            let _ = self.auth_store.get_key(provider_id.as_str()).await?;
        }
        Ok(())
    }

    fn resolve_model(&mut self, shared_auth: Arc<tokio::sync::Mutex<AuthStore>>) -> Result<ModelHandle> {
        if let Some(m) = self.model.take() {
            return Ok(m);
        }
        resolve_model_or_fallback((&mut self.config, &self.auth_store, shared_auth))
    }

    fn into_engine(
        self,
        (session_manager, auth_store): (SessionManager, Arc<tokio::sync::Mutex<AuthStore>>),
        (tools, model, agent): (Vec<DynamicTool>, ModelHandle, rig::agent::Agent),
    ) -> AgentEngine {
        let tool_names = tools.iter().map(|t| t.name().to_string()).collect();
        let context_limit = super::model::resolve_context_limit(&self.config);
        AgentEngine {
            config: self.config,
            session_manager,
            tools,
            tool_names: Arc::new(std::sync::RwLock::new(tool_names)),
            plugins: self.plugins,
            agent: Arc::new(tokio::sync::RwLock::new(agent)),
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context: ContextTracker::new(context_limit),
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
            auth_store,
            model: Some(model),
        }
    }

    pub async fn build(mut self) -> Result<AgentEngine> {
        let base_dir = self.base_dir.take().map(Ok).unwrap_or_else(std::env::current_dir)?;
        let session_manager = self.resolve_session().await?;
        self.validate_provider_auth().await?;
        let shared_auth = Arc::new(tokio::sync::Mutex::new(self.auth_store.clone()));
        let model = self.resolve_model(shared_auth.clone())?;

        let tools = build_engine_tools(
            (&base_dir, &self.config),
            self.rig_tools.take(),
            std::mem::take(&mut self.extra_tools),
        )
        .await?;
        let agent = super::runtime::build_coding_agent(
            model.clone(),
            &self.config,
            CodingRuntime {
                base_dir: &base_dir,
                memory: session_manager.clone(),
                built_in_tools: Some(tools.clone()),
            },
        )?;

        Ok(self.into_engine((session_manager, shared_auth), (tools, model, agent)))
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
        "antigravity" | "google-antigravity" => "gemini-2.5-flash",
        _ => "claude-3-7-sonnet-20250219",
    }
}
