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
use rho_harness_core::config::ModelSlot;
use rho_harness_core::error::Result;
use std::path::Path;
use std::sync::Arc;

fn build_workspace_native_tools(base_dir: &Path, lsp: Option<Arc<LspManager>>) -> Vec<Arc<dyn EngineTool>> {
    let history = EditHistory::default();
    let write = Arc::new(
        WriteTool::new(base_dir)
            .with_lsp(lsp.clone())
            .with_history(Some(history.clone())),
    );
    let edit = Arc::new(
        EditTool::new(base_dir)
            .with_lsp(lsp.clone())
            .with_history(Some(history.clone())),
    );
    let undo = Arc::new(UndoTool::new(base_dir, history.clone()).with_lsp(lsp.clone()));
    let read = Arc::new(
        ReadTool::new(base_dir)
            .with_artifact_dir(Some(base_dir.join(".rho/artifacts")))
            .with_lsp(lsp.clone()),
    );
    let bash = Arc::new(BashTool::new(base_dir));
    let fd = Arc::new(FdTool::new(base_dir));
    let rg = Arc::new(RgTool::new(base_dir));

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
        tools.push(Arc::new(LspTool::new(base_dir, lsp).with_history(Some(history))) as Arc<dyn EngineTool>);
    }
    tools
}

pub fn build_builtin_tools(
    base_dir: &Path,
    config: &Config,
    model: Option<crate::engine::compactor::llm::ModelHandle>,
) -> Result<Vec<Arc<dyn EngineTool>>> {
    build_builtin_tools_with_resolver(base_dir, config, model, None)
}

pub fn build_builtin_tools_with_resolver(
    base_dir: &Path,
    config: &Config,
    model: Option<crate::engine::compactor::llm::ModelHandle>,
    model_resolver: Option<SubagentModelResolver>,
) -> Result<Vec<Arc<dyn EngineTool>>> {
    build_builtin_toolset(
        base_dir,
        config,
        model,
        model_resolver,
        ToolsetServices::default(),
        &SubagentRuntime::default(),
    )
    .map(|(tools, _)| tools)
}

/// Long-lived services the engine shares with its tools.
#[derive(Default)]
pub struct ToolsetServices {
    /// With a store, every subagent works in its own worktree and the `worktree` tool reviews and merges them.
    pub worktrees: Option<crate::worktree::WorktreeStore>,
    /// Language servers behind the `lsp` tool, `read` symbols, and edit diagnostics.
    pub lsp: Option<Arc<LspManager>>,
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
    let mut tools = build_workspace_native_tools(base_dir, services.lsp);
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
                },
            );
            tools.push(Arc::new(search));
        }
        if config.tools.web.fetch.enabled {
            let fetch = WebFetchTool::new(
                http,
                FetchCache::new(60, 64),
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
        .with_keep_recent_tokens(config.keep_recent_tokens)
        .with_foreground_budget(std::time::Duration::from_secs(
            config
                .subagents
                .foreground_seconds
                .unwrap_or(crate::tools::subagent::DEFAULT_FOREGROUND_SECONDS),
        ));
    if let Some(resolver) = model_resolver {
        subagent = subagent.with_model_resolver(resolver).with_model_roles(
            config.slot_model(ModelSlot::Smol).map(str::to_string),
            config.slot_model(ModelSlot::Slow).map(str::to_string),
        );
    }
    if has_lsp {
        subagent = subagent.with_worktree_lsp(Arc::new(LspManager::for_workspace));
    }
    let (root, session_lsp) = (base_dir.to_path_buf(), shared_lsp);
    subagent = subagent.with_fresh_session_state(Arc::new(move |tools| {
        fresh_session_state(tools, &root, session_lsp.clone())
    }));
    if let Some(store) = services.worktrees {
        subagent = subagent.with_worktrees(store.clone());
        tools.push(Arc::new(crate::tools::WorktreeTool::new(base_dir, store)) as Arc<dyn EngineTool>);
    }
    if let Some(memory) = memory_tool(rho_harness_core::config::home_dir().as_deref(), base_dir, config) {
        tools.push(memory);
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
    let native = build_workspace_native_tools(root, lsp);
    let fresh = tools
        .iter()
        .filter_map(|tool| match native.iter().find(|n| n.name() == tool.name()) {
            Some(native) => Some(native.clone()),
            None if tool.name() == "lsp" => None,
            None => Some(tool.clone()),
        })
        .collect();
    crate::tools::subagent::lsp::restrict_lsp(fresh)
}
