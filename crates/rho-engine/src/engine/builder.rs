use super::AgentEngine;
use super::tracking::{QuotaTracker, UsageTracker};
use crate::auth::AuthStore;
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::ModelRouter;
use crate::tools::EngineTool;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::session::SessionManager;
use std::path::{Path, PathBuf};
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
    quota_endpoints: super::quota_refresh::QuotaEndpoints,
}

struct EngineAssembly {
    tools: Arc<tokio::sync::RwLock<Vec<Arc<dyn EngineTool>>>>,
    tool_names: Vec<String>,
    model: Option<ModelHandle>,
}

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
            model_catalog: crate::provider::ModelCatalog::default(),
            quota_endpoints: super::quota_refresh::QuotaEndpoints::default(),
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

    pub fn quota_endpoints(mut self, endpoints: super::quota_refresh::QuotaEndpoints) -> Self {
        self.quota_endpoints = endpoints;
        self
    }
}

/// Subagent model roles; a spec without a provider uses the session's provider.
fn subagent_model_resolver(router: &ModelRouter, config: &Config) -> crate::tools::subagent::SubagentModelResolver {
    let router = router.clone();
    let config = short_lived_config(config);
    Arc::new(move |spec: &str| {
        let route = ModelRoute::parse(spec, None, &config.route().connection.to_string());
        router.build_blocking(&config, &route).map_err(|e| e.to_string())
    })
}

fn short_lived_config(config: &Config) -> Config {
    Config {
        cache_retention: config.cache_retention.for_short_lived_work(),
        ..config.clone()
    }
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

    /// The session model built with short-lived cache retention for subagents that run on it; `None`
    /// when the model was supplied directly or none is selected.
    async fn resolve_subagent_default(&self, router: &ModelRouter) -> Result<Option<ModelHandle>> {
        if self.model.is_some() || self.config.model.trim().is_empty() {
            return Ok(None);
        }
        let config = short_lived_config(&self.config);
        router.build(&config, &config.route()).await.map(Some)
    }

    /// `None` until a model is selected: an empty `config.model` means none was chosen or saved.
    async fn resolve_model(&mut self, router: &ModelRouter) -> Result<Option<ModelHandle>> {
        if let Some(m) = self.model.take() {
            return Ok(Some(m));
        }
        if self.config.model.trim().is_empty() {
            return Ok(None);
        }
        router.build(&self.config, &self.config.route()).await.map(Some)
    }

    fn into_engine(
        self,
        base_dir: PathBuf,
        session_manager: SessionManager,
        router: ModelRouter,
        assembly: EngineAssembly,
    ) -> AgentEngine {
        let EngineAssembly {
            tools,
            tool_names,
            model,
        } = assembly;
        let context = super::tracking::ContextTracker::new(self.config.context_limit);
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
            quota_endpoints: self.quota_endpoints,
            context,
            run_tracker: super::metrics::RunTracker::default(),
            project_context: Arc::default(),
            router,
            model: model.clone(),
            compactor_model: model,
            demotion_hook,
            agent_monitor: self.agent_monitor,
            subagent_runtime: self.subagent_runtime,
            turn_notes: self.turn_notes,
            prune_memory: std::sync::Mutex::default(),
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
        let router = ModelRouter::new(shared_auth, self.model_catalog.clone());
        let subagent_default = self.resolve_subagent_default(&router).await?;
        let model = self.resolve_model(&router).await?;

        let history_messages = session_manager.active_messages().await.unwrap_or_default();
        let history_tools = crate::mcp::extract_invoked_tool_names(&history_messages);

        let EngineTools { tools, subagents, mcp } = build_engine_tools(
            &base_dir,
            &self.config,
            SubagentModels {
                default: subagent_default.as_ref().or(model.as_ref()),
                resolver: subagent_model_resolver(&router, &self.config),
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
        let assembly = EngineAssembly {
            tools: tools_lock,
            tool_names,
            model,
        };
        let mut engine = self.into_engine(base_dir, session_manager, router, assembly);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_auth_store;

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

    #[test]
    fn subagents_get_short_retention_only_under_auto() {
        use rho_harness_core::config::CacheRetention::{Auto, Long, Short};
        for (configured, expected) in [(Auto, Short), (Short, Short), (Long, Long)] {
            let config = Config {
                cache_retention: configured,
                ..Config::default()
            };
            assert_eq!(short_lived_config(&config).cache_retention, expected, "{configured}");
        }
    }

    #[tokio::test]
    async fn subagent_default_model_needs_a_selected_config_model() {
        let (_auth_dir, store) = temp_auth_store();
        let router = ModelRouter::new(
            Arc::new(tokio::sync::Mutex::new(store.clone())),
            crate::provider::ModelCatalog::default(),
        );
        let builder = AgentEngineBuilder::new(Config::default(), store);
        assert!(builder.resolve_subagent_default(&router).await.unwrap().is_none());
    }
}
