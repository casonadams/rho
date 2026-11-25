//! Agent engine: `AgentEngine` owns the turn loop (prepare → stream → tool
//! dispatch → completion), usage tracking, and auto-compaction. Submodules:
//! `builder` (construction), `runner` (turn/sink execution), `context`
//! (instruction discovery), `compactor` (summarization), `tracking` (usage),
//! `metrics`, `runtime`, and `eval` (deterministic mock harness).

pub use crate::repeat;
pub use crate::repeat::REPEATED_CALL_MESSAGE;
use std::path::Path;
pub use tracking::{SessionUsageTotals, SpeedTracker};
pub mod builder;
pub use builder::AgentEngineBuilder;
pub mod compactor;
pub mod context;
mod display;
pub mod ephemeral;
pub use ephemeral::{
    EphemeralTurnRequest, EphemeralTurnResponse, PreparedEphemeralTurn, build_recap_prompt, format_recap_text,
};
mod locking;
pub mod metrics;
mod model;
mod notes;
pub use notes::TurnNotes;
pub(crate) mod quota_refresh;
pub mod runner;
pub mod title;
pub mod tracking;

pub use compactor::CompactionStats;

#[cfg(test)]
mod tests;

pub mod eval;

use crate::auth::AuthStore;
use crate::tools::EngineTool;
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::session::SessionManager;
use std::sync::Arc;
use tracking::{ContextTracker, QuotaTracker, UsageTracker};

pub struct AgentEngine {
    pub config: Config,
    pub base_dir: std::path::PathBuf,
    pub session_manager: SessionManager,
    pub(crate) tool_names: Arc<std::sync::RwLock<Vec<String>>>,
    pub(crate) tools: Arc<tokio::sync::RwLock<Vec<Arc<dyn EngineTool>>>>,
    pub(crate) usage: UsageTracker,
    pub(crate) quota: QuotaTracker,
    pub(crate) quota_endpoints: quota_refresh::QuotaEndpoints,
    pub(crate) title_tracker: title::TitleTracker,
    pub(crate) context: ContextTracker,
    pub(crate) run_tracker: metrics::RunTracker,
    pub(crate) project_context: Arc<tokio::sync::Mutex<Option<(std::path::PathBuf, context::ProjectContext)>>>,
    pub(crate) router: crate::provider::ModelRouter,
    pub(crate) model: Option<crate::engine::compactor::llm::ModelHandle>,
    pub(crate) compactor_model: Option<crate::engine::compactor::llm::ModelHandle>,
    pub(crate) demotion_hook: Option<Arc<dyn crate::engine::compactor::DemotionHook>>,
    pub(crate) agent_monitor: rho_harness_core::agents::AgentMonitor,
    pub(crate) subagent_runtime: crate::tools::subagent::SubagentRuntime,
    pub(crate) turn_notes: TurnNotes,
    /// Tool-result stubs sent in the last request, replayed while the prompt cache is warm.
    pub(crate) prune_memory: std::sync::Mutex<runner::PruneMemory>,
    pub(crate) workflows: Option<crate::tools::workflow::WorkflowRunner>,
    /// Shared with the subagent tool so `update_model` moves subagents to the new model and thinking level.
    pub(crate) subagent_models: Option<crate::tools::subagent::SubagentModels>,
    /// Where subagent and workflow worktrees live; the parent is reminded to merge or discard its own.
    pub(crate) worktrees: Option<crate::worktree::WorktreeStore>,
    pub(crate) pending_relocation: Arc<std::sync::Mutex<Option<std::path::PathBuf>>>,
    pub(crate) mcp: EngineMcp,
    pub(crate) ollama_host: String,
    pub(crate) presenter: Option<Arc<dyn rho_harness_core::presentation::Presenter>>,
}

#[derive(Default)]
pub(crate) struct EngineMcp {
    pub(crate) diagnostics: Vec<crate::mcp::McpDiagnostic>,
    pub(crate) instructions: Vec<crate::mcp::McpServerInstructions>,
    pub(crate) runtime: Option<Arc<crate::mcp::McpRuntime>>,
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

    pub fn mcp_diagnostics(&self) -> &[crate::mcp::McpDiagnostic] {
        &self.mcp.diagnostics
    }

    pub fn mcp_runtime(&self) -> Option<&Arc<crate::mcp::McpRuntime>> {
        self.mcp.runtime.as_ref()
    }

    pub fn tools(&self) -> Arc<tokio::sync::RwLock<Vec<Arc<dyn EngineTool>>>> {
        Arc::clone(&self.tools)
    }

    pub fn agent_monitor(&self) -> &rho_harness_core::agents::AgentMonitor {
        &self.agent_monitor
    }

    /// Sends `text` from the user to the running round of subagent task `task_id`; `false` when that
    /// round has already ended.
    pub fn message_running_subagent(&self, task_id: &str, text: &str) -> bool {
        self.subagent_runtime.message_from_user(task_id, text)
    }

    /// The top-level agent's mailbox: background notices and messages from its subagents wait here
    /// until a turn reads them, or the idle prompt hands them to the agent.
    pub fn inbox(&self) -> crate::mailbox::Mailbox {
        self.subagent_runtime.inbox(&self.session_manager.session_id)
    }

    /// Asks subagent task `task_id`'s running round to stop and save its work; `false` when none runs.
    pub fn stop_subagent(&self, task_id: &str) -> bool {
        self.subagent_runtime.stop(task_id)
    }

    /// Replaces this engine with `next`. Leaving a different session stops its background work, since
    /// nothing could deliver the results; rebuilding the same session keeps it.
    pub fn switch_to(&mut self, next: Self) {
        if self.session_manager.session_id != next.session_manager.session_id {
            self.cancel_auto_title();
            crate::tools::jobs::abandon_background_work(&self.session_manager.session_id);
        }
        *self = next;
    }

    /// Shows the main agent a workflow's report, with its agent task ids, at the start of the next turn.
    pub fn hand_off_workflow_report(&self, name: &str, report: &str) {
        self.turn_notes.push(format!(
            "[The user ran workflow `{name}`; its report follows. Its changes are in a git worktree, not the working tree. Verify its work before relying on it: review the worktree's diff and re-run the relevant checks there. Inspect any listed agent task with the subagent tool's `task_id` and `inspect: true`, continue it with `task_id` and feedback if something is missing or wrong, and merge or discard the worktree with the worktree tool when done.]\n{report}"
        ));
    }

    pub async fn workflow_runner(
        &self,
        progress: rho_harness_core::presentation::ToolStreamPort,
        cancellation: crate::engine::runner::turn::types::CancellationSignal,
    ) -> std::result::Result<crate::tools::workflow::WorkflowRunner, String> {
        let runner = self
            .workflows
            .as_ref()
            .ok_or_else(|| "workflows are not available in this session".to_string())?;
        let project = self.project_context().await.ok().map(Arc::new);
        Ok(runner.bind(&crate::tools::engine_tool::TurnToolContext {
            cancellation: Some(cancellation),
            progress,
            monitor: Some(self.agent_monitor.clone()),
            project,
            session: Some(self.session_manager.clone()),
            inbox: Some(self.inbox()),
            ..Default::default()
        }))
    }

    pub fn shared_auth_store(&self) -> Arc<tokio::sync::Mutex<AuthStore>> {
        Arc::clone(self.router.auth())
    }

    pub async fn rebuild(&self, config: Config, auth_store: AuthStore) -> Result<Self> {
        let mut builder = builder::AgentEngineBuilder::new(config, auth_store)
            .session(self.session_manager.clone())
            .agent_monitor(self.agent_monitor.clone())
            .subagent_runtime(self.subagent_runtime.clone())
            .turn_notes(self.turn_notes.clone())
            .model_catalog(self.model_catalog().clone())
            .base_dir(self.base_dir.clone());
        if let Some(hook) = &self.demotion_hook {
            builder = builder.demotion_hook(Arc::clone(hook));
        }
        if let Some(presenter) = &self.presenter {
            builder = builder.presenter(Arc::clone(presenter));
        }
        let rebuilt = builder.build().await?;
        rebuilt.spawn_refresh_quota();
        Ok(rebuilt)
    }

    /// Returns and clears any pending working-directory relocation requested by `worktree(action="create")`.
    pub fn take_pending_relocation(&self) -> Option<std::path::PathBuf> {
        self.pending_relocation.lock().ok()?.take()
    }

    /// Relocates the live session into `new_base_dir` without dropping conversation history.
    pub async fn relocate_working_dir(&mut self, new_base_dir: std::path::PathBuf) -> Result<()> {
        let _ = std::env::set_current_dir(&new_base_dir);
        let _ = rho_harness_core::session::record_session_for_cwd(
            &self.config.sessions_dir,
            &new_base_dir,
            &self.session_manager.session_id,
        );
        let auth_store = self.router.auth().lock().await.clone();
        let mut builder = builder::AgentEngineBuilder::new(self.config.clone(), auth_store)
            .session(self.session_manager.clone())
            .agent_monitor(self.agent_monitor.clone())
            .subagent_runtime(self.subagent_runtime.clone())
            .turn_notes(self.turn_notes.clone())
            .model_catalog(self.model_catalog().clone())
            .base_dir(new_base_dir);
        if let Some(hook) = &self.demotion_hook {
            builder = builder.demotion_hook(Arc::clone(hook));
        }
        if let Some(presenter) = &self.presenter {
            builder = builder.presenter(Arc::clone(presenter));
        }
        let rebuilt = builder.build().await?;
        rebuilt.spawn_refresh_quota();
        self.switch_to(rebuilt);
        Ok(())
    }

    /// Creates a managed session worktree on `branch` (or `wt/<yyyymmdd-hhmmss>`) and relocates this session into it.
    pub async fn enter_session_worktree(&mut self, branch: Option<&str>) -> Result<crate::worktree::SessionWorktree> {
        let branch = branch
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map_or_else(crate::worktree::default_session_branch, str::to_string);
        let base_dir = self.config.worktree.effective_base(&self.config.config_dir);
        let created = crate::worktree::create_session_worktree(
            &self.base_dir,
            &base_dir,
            &branch,
            self.config.worktree.clean_source,
        )
        .await
        .map_err(|msg| AppError::Other(anyhow::anyhow!("{msg}")))?;
        self.relocate_working_dir(created.path.clone()).await?;
        Ok(created)
    }

    fn build_cached_context_dirs<'a>(
        &'a self,
        tools: &'a [String],
        home_dir: Option<&'a Path>,
    ) -> context::ContextDirs<'a> {
        context::ContextDirs {
            config_dir: Some(&self.config.config_dir),
            home_dir,
            system_prompt: self.config.system_prompt.as_deref(),
            append_system_prompt: self.config.append_system_prompt.as_deref(),
            active_tools: Some(tools),
            no_context_files: self.config.no_context_files,
        }
    }

    pub async fn project_context(&self) -> Result<context::ProjectContext> {
        let cwd = std::env::current_dir()?;
        let mut cache = self.project_context.lock().await;
        if cache.as_ref().map(|(dir, _)| dir.as_path()) != Some(cwd.as_path()) {
            let tools = self.tool_names();
            let home = rho_harness_core::config::home_dir();
            let dirs = self.build_cached_context_dirs(&tools, home.as_deref());
            *cache = Some((
                cwd.clone(),
                context::ProjectContext::discover_with_dirs(&cwd, dirs).await,
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
}
