pub mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_BASH, PROMPT_EDIT, PROMPT_FD, PROMPT_READ, PROMPT_RG,
    PROMPT_WEB_FETCH, PROMPT_WEB_SEARCH, PROMPT_WRITE,
};

use crate::lsp::LspManager;
use crate::tools::bash::BashTool;
use crate::tools::edit::EditTool;
use crate::tools::engine_tool::EngineTool;
use crate::tools::fd::FdTool;
use crate::tools::lsp::LspTool;
use crate::tools::read::ReadTool;
use crate::tools::rg::RgTool;
use crate::tools::subagent::lsp::WorktreeLsp;
use crate::tools::subagent::{SubagentModelResolver, SubagentRuntime, SubagentTool};
use crate::tools::todo::TodoTool;
use crate::tools::undo::{EditHistory, UndoTool};
use crate::tools::wait::WaitTool;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::WriteTool;
use rho_harness_core::config::Config;
use rho_harness_core::config::ModelRole;
use rho_harness_core::error::Result;
use std::path::Path;
use std::sync::Arc;

fn build_workspace_native_tools(
    workspace: crate::tools::WorkspaceDir,
    lsp: Option<Arc<LspManager>>,
    direnv: crate::tools::bash::DirenvOptions,
) -> Vec<Arc<dyn EngineTool>> {
    let base_dir = workspace.get();
    let history = EditHistory::default();
    let write = Arc::new(
        WriteTool::new(&base_dir)
            .with_workspace_dir(workspace.clone())
            .with_lsp(lsp.clone())
            .with_history(Some(history.clone())),
    );
    let edit = Arc::new(
        EditTool::new(&base_dir)
            .with_workspace_dir(workspace.clone())
            .with_lsp(lsp.clone())
            .with_history(Some(history.clone())),
    );
    let undo = Arc::new(
        UndoTool::new(&base_dir, history.clone())
            .with_workspace_dir(workspace.clone())
            .with_lsp(lsp.clone()),
    );
    let read = Arc::new(
        ReadTool::new(&base_dir)
            .with_workspace_dir(workspace.clone())
            .with_lsp(lsp.clone()),
    );
    let bash = Arc::new(
        BashTool::new(&base_dir)
            .with_workspace_dir(workspace.clone())
            .with_direnv(direnv),
    );
    let fd = Arc::new(FdTool::new(&base_dir).with_workspace_dir(workspace.clone()));
    let rg = Arc::new(RgTool::new(&base_dir).with_workspace_dir(workspace.clone()));

    let mut tools = vec![
        read as Arc<dyn EngineTool>,
        write as Arc<dyn EngineTool>,
        edit as Arc<dyn EngineTool>,
        undo as Arc<dyn EngineTool>,
        bash as Arc<dyn EngineTool>,
        fd as Arc<dyn EngineTool>,
        rg as Arc<dyn EngineTool>,
    ];
    if let Some(lsp) = lsp {
        tools.push(Arc::new(
            LspTool::new(&base_dir, lsp)
                .with_workspace_dir(workspace)
                .with_history(Some(history)),
        ) as Arc<dyn EngineTool>);
    }
    tools
}

/// The built-in tools and the subagent runtime their `subagent` tool runs under. An engine built
/// from them takes the same runtime, so it reaches the subagents and notices those tools produce.
#[derive(Clone)]
pub struct BuiltinTools {
    pub tools: Vec<Arc<dyn EngineTool>>,
    pub runtime: SubagentRuntime,
}

pub fn build_builtin_tools(
    base_dir: &Path,
    config: &Config,
    model: Option<crate::engine::compactor::llm::ModelHandle>,
) -> Result<BuiltinTools> {
    let runtime = SubagentRuntime::default();
    let (tools, _) = build_builtin_toolset(base_dir, config, model, None, ToolsetServices::default(), &runtime)?;
    Ok(BuiltinTools { tools, runtime })
}

/// Long-lived services the engine shares with its tools.
#[derive(Default)]
pub struct ToolsetServices {
    /// With a store, every subagent works in its own worktree and the `worktree` tool reviews and merges them.
    pub worktrees: Option<crate::worktree::WorktreeStore>,
    /// Language servers behind the `lsp` tool, `read` symbols, and edit diagnostics.
    pub lsp: Option<Arc<LspManager>>,
    /// Shared slot populated when `worktree(action="create")` requests live session relocation.
    pub relocate_slot: Option<Arc<std::sync::Mutex<Option<std::path::PathBuf>>>>,
    /// Presenter for interactive tools like `ask` (registered only when `has_interactive_ui()` is true).
    pub presenter: Option<Arc<dyn rho_harness_core::presentation::Presenter>>,
    /// Optional decision model handle for `ask` tool's `decision_assist`.
    pub ask_decision: Option<crate::decision::DecisionHandle>,
}

/// The built-in tools and the subagent tool.
pub fn build_builtin_toolset(
    base_dir: &Path,
    config: &Config,
    model: Option<crate::engine::compactor::llm::ModelHandle>,
    model_resolver: Option<SubagentModelResolver>,
    services: ToolsetServices,
    runtime: &SubagentRuntime,
) -> Result<(Vec<Arc<dyn EngineTool>>, SubagentTool)> {
    let has_lsp = services.lsp.is_some();
    let shared_lsp = services.lsp.clone();
    let direnv = crate::tools::bash::DirenvOptions {
        enabled: config.tools.bash.direnv,
        timeout_ms: config.tools.bash.direnv_timeout_ms,
    };
    let workspace = crate::tools::WorkspaceDir::new(base_dir);
    let mut tools = build_workspace_native_tools(workspace.clone(), services.lsp, direnv);
    if config.tools.web.search.enabled || config.tools.web.fetch.enabled {
        let http = HttpClient::new(config.allow_private_network)?;
        if config.tools.web.search.enabled {
            let engines = crate::tools::web::search::resolve_engine_chain(
                &config.tools.web.search.default,
                &config.tools.web.search.fallback,
            )?;
            let search = WebSearchTool::new(
                http.clone(),
                SearchRateLimiter::new(config.search_min_interval_ms),
                WebSearchConfig {
                    region: config.region.clone(),
                    timeout_sec: config.search_timeout_sec,
                    engines,
                    auth_file: Some(config.auth_file.clone()),
                },
            );
            tools.push(Arc::new(search));
        }
        if config.tools.web.fetch.enabled {
            let fetch = WebFetchTool::new(
                http,
                FetchCache::new(60, 64, 32 * 1024 * 1024),
                WebFetchConfig {
                    timeout_sec: config.fetch_timeout_sec,
                    max_bytes: config.fetch_max_bytes,
                    pdf_max_bytes: 30 * 1024 * 1024,
                    default_limit: config.fetch_limit,
                },
            );
            tools.push(Arc::new(fetch));
        }
    }
    tools.push(Arc::new(TodoTool::default()));
    tools.push(Arc::new(WaitTool::default()));
    let resolved_agents = rho_harness_core::agents::resolved_agents(Some(base_dir));
    let agent_metas: Vec<_> = resolved_agents.into_iter().map(|a| a.metadata).collect();
    let mut subagent = crate::tools::subagent::SubagentTool::new(model, tools.clone())
        .with_agents(agent_metas)
        .with_personas(rho_harness_core::agents::resolved_personas(Some(base_dir)))
        .with_runtime(runtime)
        .with_pinned_models(config.subagents.models.clone())
        .with_foreground_budget(std::time::Duration::from_secs(
            config
                .subagents
                .foreground_seconds
                .unwrap_or(crate::tools::subagent::DEFAULT_FOREGROUND_SECONDS),
        ));
    if let Some(resolver) = model_resolver {
        subagent = subagent.with_model_resolver(resolver).with_model_roles(
            config.role_model(ModelRole::SMOL.as_str()).map(str::to_string),
            config.role_model(ModelRole::SLOW.as_str()).map(str::to_string),
        );
    }
    if has_lsp {
        subagent = subagent.with_worktree_lsp(Arc::new(LspManager::for_workspace));
    }
    let (subagent_workspace, session_lsp) = (workspace.clone(), shared_lsp);
    subagent = subagent.with_fresh_session_state(Arc::new(move |tools| {
        let current_root = subagent_workspace.get();
        fresh_session_state(tools, &current_root, session_lsp.clone())
    }));
    if let Some(store) = services.worktrees {
        subagent = subagent.with_worktrees(store.clone());
        let wt_base = config.worktree.effective_base(&config.config_dir);
        let wt_tool = crate::tools::WorktreeTool::new(base_dir, store)
            .with_workspace_dir(workspace)
            .with_session_worktree(wt_base, config.worktree.clean_source, services.relocate_slot);
        tools.push(Arc::new(wt_tool) as Arc<dyn EngineTool>);
    }
    if let Some(memory) = memory_tool(rho_harness_core::config::home_dir().as_deref(), base_dir, config) {
        tools.push(memory);
    }
    if let Some(presenter) = services.presenter
        && presenter.has_interactive_ui()
    {
        let mut ask = crate::tools::AskTool::new(presenter);
        if let Some(decision) = services.ask_decision {
            ask = ask.with_decision(decision);
        }
        tools.push(Arc::new(ask) as Arc<dyn EngineTool>);
    }
    tools.push(Arc::new(subagent.clone()) as Arc<dyn EngineTool>);
    Ok((tools, subagent))
}

/// The memory tool, unless there is no home to keep memory in or context files are disabled.
fn memory_tool(home: Option<&Path>, base_dir: &Path, config: &Config) -> Option<Arc<dyn EngineTool>> {
    let home = home.filter(|_| !config.no_context_files)?;
    let store = rho_harness_core::memory::MemoryStore::in_home(home, base_dir);
    Some(Arc::new(crate::tools::MemoryTool::new(store)))
}

/// `tools` with the workspace-native ones (read, write, edit, undo, bash, fd, rg) rooted at
/// `root` instead. With `lsp`, they and the `lsp` tool use the manager it opens for `root`;
/// without it they run without language servers.
pub(crate) fn rerooted(
    tools: &[Arc<dyn EngineTool>],
    root: &Path,
    lsp: Option<&WorktreeLsp>,
) -> Vec<Arc<dyn EngineTool>> {
    fresh_session_state(tools, root, lsp.map(|open| open(root)))
}

/// `tools` with the ones that share an edit history (write, edit, undo and lsp) replaced by new
/// ones, so a subagent cannot undo its parent's edits. The workspace-native tools are rooted at
/// `root`; the `lsp` tool uses `lsp`, and is dropped without it.
pub(crate) fn fresh_session_state(
    tools: &[Arc<dyn EngineTool>],
    root: &Path,
    lsp: Option<Arc<LspManager>>,
) -> Vec<Arc<dyn EngineTool>> {
    let native = build_workspace_native_tools(
        crate::tools::WorkspaceDir::new(root),
        lsp,
        crate::tools::bash::DirenvOptions::default(),
    );
    let fresh = tools
        .iter()
        .filter_map(|tool| match native.iter().find(|n| n.name() == tool.name()) {
            Some(native) => Some(native.clone()),
            None if matches!(tool.name(), "lsp" | "ask") => None,
            None => Some(tool.clone()),
        })
        .collect();
    crate::tools::subagent::lsp::restrict_lsp(fresh)
}
