use super::AgentEngine;
use super::tracking::{QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use crate::engine::compactor::llm::ModelHandle;
use crate::tools::EngineTool;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionManager;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

pub struct AgentEngineBuilder {
    config: Config,
    auth_store: AuthStore,
    resume_id: Option<String>,
    session_manager: Option<SessionManager>,
    base_dir: Option<PathBuf>,
    tools: Option<Vec<Arc<dyn EngineTool>>>,
    extra_tools: Vec<Arc<dyn EngineTool>>,
    model: Option<ModelHandle>,
    demotion_hook: Option<Arc<dyn crate::engine::compactor::DemotionHook>>,
    agent_monitor: rho_harness_core::agents::AgentMonitor,
    subagent_runtime: crate::tools::subagent::SubagentRuntime,
    turn_notes: super::TurnNotes,
    model_catalog: crate::provider::ModelCatalog,
}

type EngineAssembly = (
    Arc<tokio::sync::RwLock<Vec<Arc<dyn EngineTool>>>>,
    Vec<String>,
    Option<ModelHandle>,
);

impl AgentEngineBuilder {
    pub fn new(config: Config, auth_store: AuthStore) -> Self {
        Self {
            tools: None,
            extra_tools: Vec::new(),
            config,
            auth_store,
            resume_id: None,
            session_manager: None,
            base_dir: None,
            model: None,
            demotion_hook: None,
            agent_monitor: rho_harness_core::agents::AgentMonitor::default(),
            subagent_runtime: crate::tools::subagent::SubagentRuntime::default(),
            turn_notes: super::TurnNotes::default(),
            model_catalog: crate::provider::ModelCatalog::shared(),
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

    pub fn tools(mut self, tools: Vec<Arc<dyn EngineTool>>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn add_tool(mut self, tool: Arc<dyn EngineTool>) -> Self {
        self.extra_tools.push(tool);
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

    pub fn demotion_hook(mut self, hook: Arc<dyn crate::engine::compactor::DemotionHook>) -> Self {
        self.demotion_hook = Some(hook);
        self
    }

    pub fn agent_monitor(mut self, monitor: rho_harness_core::agents::AgentMonitor) -> Self {
        self.agent_monitor = monitor;
        self
    }

    pub fn subagent_runtime(mut self, runtime: crate::tools::subagent::SubagentRuntime) -> Self {
        self.subagent_runtime = runtime;
        self
    }

    pub fn turn_notes(mut self, notes: super::TurnNotes) -> Self {
        self.turn_notes = notes;
        self
    }

    pub fn model_catalog(mut self, catalog: crate::provider::ModelCatalog) -> Self {
        self.model_catalog = catalog;
        self
    }
}

async fn resolve_model_off_runtime(
    config: Config,
    auth_store: AuthStore,
    shared_auth: Arc<tokio::sync::Mutex<AuthStore>>,
) -> Result<ModelHandle> {
    tokio::task::spawn_blocking(move || create_engine_model(&config, &auth_store, shared_auth))
        .await
        .map_err(|e| AppError::Auth(format!("Model resolution task failed: {e}")))?
}

fn subagent_model_resolver(
    config: &Config,
    auth_store: &AuthStore,
    shared_auth: Arc<tokio::sync::Mutex<AuthStore>>,
) -> crate::tools::subagent::SubagentModelResolver {
    let config = config.clone();
    let auth_store = auth_store.clone();
    Arc::new(move |model: &str| {
        crate::provider::ProviderFactory::create_model(&config, model, &auth_store, shared_auth.clone())
            .map_err(|e| e.to_string())
    })
}

struct SubagentModels<'a> {
    default: Option<&'a ModelHandle>,
    resolver: crate::tools::subagent::SubagentModelResolver,
    runtime: &'a crate::tools::subagent::SubagentRuntime,
}

async fn build_engine_tools(
    base_dir: &Path,
    config: &Config,
    models: SubagentModels<'_>,
    tools_opt: Option<Vec<Arc<dyn EngineTool>>>,
    extra_tools: Vec<Arc<dyn EngineTool>>,
    history_tools: &std::collections::HashSet<String>,
) -> Result<EngineTools> {
    let mut mcp = crate::mcp::load_mcp(config, base_dir, history_tools).await;
    let mcp_tools = std::mem::take(&mut mcp.tools);

    let (mut tools, subagents) = match tools_opt {
        Some(t) => (t, None),
        None => {
            let (mut t, subagents) = crate::tools::builtin_tools::build_builtin_toolset(
                base_dir,
                config,
                models.default.cloned(),
                Some(models.resolver),
                Some(crate::worktree::WorktreeStore::default_location()),
                models.runtime,
            )?;
            t.extend(mcp_tools);
            (t, Some(subagents))
        }
    };
    tools.extend(extra_tools);
    tools.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(EngineTools { tools, subagents, mcp })
}

struct EngineTools {
    tools: Vec<Arc<dyn EngineTool>>,
    subagents: Option<crate::tools::subagent::SubagentTool>,
    mcp: crate::mcp::McpLoad,
}

impl AgentEngineBuilder {
    async fn resolve_session(&self) -> Result<SessionManager> {
        let session_manager = match self.session_manager.clone() {
            Some(session) => session,
            None => {
                SessionManager::new_with_secrets_async(
                    &self.config.sessions_dir,
                    self.resume_id.as_deref(),
                    self.auth_store.resolved_secret_values().await,
                )
                .await?
            }
        };
        if let Some(days) = self.config.session_retention_days
            && days > 0
        {
            session_manager.spawn_auto_prune(days);
        }
        static BASH_LOG_SWEEP: std::sync::Once = std::sync::Once::new();
        BASH_LOG_SWEEP.call_once(|| {
            tokio::task::spawn_blocking(|| crate::tools::bash::accumulator::sweep_logs(&std::env::temp_dir()));
        });
        Ok(session_manager)
    }

    /// `None` until a model is selected: an empty `config.model` means none was chosen or saved.
    async fn resolve_model(&mut self, shared_auth: Arc<tokio::sync::Mutex<AuthStore>>) -> Result<Option<ModelHandle>> {
        if let Some(m) = self.model.take() {
            return Ok(Some(m));
        }
        if self.config.model.trim().is_empty() {
            return Ok(None);
        }
        let model = resolve_model_off_runtime(self.config.clone(), self.auth_store.clone(), shared_auth).await?;
        Ok(Some(model))
    }

    fn into_engine(
        self,
        base_dir: PathBuf,
        (session_manager, auth_store): (SessionManager, Arc<tokio::sync::Mutex<AuthStore>>),
        (tools, tool_names, model): EngineAssembly,
    ) -> AgentEngine {
        let context = super::tracking::ContextTracker::new(self.config.context_limit, self.model_catalog);
        let demotion_hook = self.demotion_hook.or_else(|| {
            Some(Arc::new(
                crate::engine::compactor::artifact_spill::ArtifactDemotionHook::new(base_dir.join(".rho/artifacts")),
            ))
        });
        AgentEngine {
            config: self.config,
            base_dir,
            session_manager,
            tool_names: Arc::new(std::sync::RwLock::new(tool_names)),
            tools,
            usage: UsageTracker::default(),
            quota: QuotaTracker::default(),
            context,
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
            auth_store,
            model: model.clone(),
            compactor_model: model,
            demotion_hook,
            agent_monitor: self.agent_monitor,
            subagent_runtime: self.subagent_runtime,
            turn_notes: self.turn_notes,
            workflows: None,
            worktrees: None,
            mcp: super::EngineMcp::default(),
            ollama_host: crate::ollama::local::local_host(),
        }
    }

    pub async fn build(mut self) -> Result<AgentEngine> {
        let base_dir = self.base_dir.take().map(Ok).unwrap_or_else(std::env::current_dir)?;
        let session_manager = self.resolve_session().await?;
        let shared_auth = Arc::new(tokio::sync::Mutex::new(self.auth_store.clone()));
        let model = self.resolve_model(shared_auth.clone()).await?;

        let history_messages = session_manager.active_messages().await.unwrap_or_default();
        let history_tools = crate::mcp::extract_invoked_tool_names(&history_messages);

        let EngineTools { tools, subagents, mcp } = build_engine_tools(
            &base_dir,
            &self.config,
            SubagentModels {
                default: model.as_ref(),
                resolver: subagent_model_resolver(&self.config, &self.auth_store, shared_auth.clone()),
                runtime: &self.subagent_runtime,
            },
            self.tools.take(),
            std::mem::take(&mut self.extra_tools),
            &history_tools,
        )
        .await?;

        let tool_names = tools.iter().map(|t| t.name().to_string()).collect::<Vec<_>>();
        let tools_lock = Arc::new(tokio::sync::RwLock::new(tools));
        let home = rho_harness_core::config::home_dir();
        let workflows = subagents.map(|s| crate::tools::workflow::WorkflowRunner::new(s, base_dir.clone(), home));
        let worktrees = workflows.as_ref().and_then(|w| w.worktrees().cloned());
        let mut engine = self.into_engine(
            base_dir,
            (session_manager, shared_auth),
            (tools_lock, tool_names, model),
        );
        engine.workflows = workflows;
        engine.worktrees = worktrees;
        engine.mcp = super::EngineMcp {
            diagnostics: mcp.diagnostics,
            instructions: mcp.instructions,
            runtime: mcp.runtime,
        };
        Ok(engine)
    }
}

fn resolve_provider_and_model(config: &Config) -> (String, String) {
    let (spec_provider, spec_model) = rho_harness_core::provider::parse_model_spec(&config.model);
    if !spec_provider.is_empty() {
        return (spec_provider, spec_model);
    }
    let provider = if !config.provider.trim().is_empty() {
        config.provider.trim().to_string()
    } else {
        rho_harness_core::provider::infer_provider_for_model(&config.model)
            .unwrap_or("local")
            .to_string()
    };
    (provider, config.model.trim().to_string())
}

pub fn create_engine_model(
    config: &Config,
    auth_store: &AuthStore,
    shared_auth: Arc<tokio::sync::Mutex<AuthStore>>,
) -> Result<ModelHandle> {
    let (provider_name, actual_model) = resolve_provider_and_model(config);
    if let Ok(provider_id) = ProviderId::from_str(&provider_name) {
        return crate::provider::ProviderFactory::create_model_for(
            crate::provider::ModelRequest {
                provider: provider_id,
                model: &actual_model,
                thinking_level: config.thinking_level.as_deref(),
                shared_auth,
            },
            auth_store,
        );
    }
    crate::provider::ProviderFactory::create_model(config, &config.model, auth_store, shared_auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_auth_store;
    use rho_harness_core::auth::StoredCredential;

    fn shared(store: &AuthStore) -> Arc<tokio::sync::Mutex<AuthStore>> {
        Arc::new(tokio::sync::Mutex::new(store.clone()))
    }

    #[tokio::test]
    async fn engine_without_a_selected_model_builds_and_refuses_turns() {
        let dir = tempfile::tempdir().unwrap();
        let (_auth_dir, store) = temp_auth_store();
        let config = Config {
            config_dir: dir.path().to_path_buf(),
            sessions_dir: dir.path().join("sessions"),
            ..Config::default()
        };
        assert!(config.model.is_empty() && config.provider.is_empty());
        let engine = AgentEngineBuilder::new(config, store)
            .base_dir(dir.path().to_path_buf())
            .build()
            .await
            .unwrap();

        let error = engine
            .run_turn(
                crate::engine::runner::TurnRequest::new("hi"),
                Arc::new(crate::engine::eval::presenter::NoopPresenter),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("No model selected"), "{error}");
        assert!(engine.session_manager.active_messages().await.unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn slow_command_key_resolution_keeps_the_runtime_responsive() {
        let (_dir, mut store) = temp_auth_store();
        store
            .set_credential(
                "anthropic",
                StoredCredential::api_key("!sleep 0.2; echo sk-ant-slow".to_string()),
            )
            .unwrap();
        let config = Config {
            provider: "anthropic".to_string(),
            model: "anthropic/claude-3.7-sonnet".to_string(),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let resolving = tokio::spawn(resolve_model_off_runtime(config, store.clone(), shared(&store)));
        tokio::task::yield_now().await;
        assert!(started.elapsed() < std::time::Duration::from_millis(150));
        let _model = resolving.await.unwrap().unwrap();
    }
}
