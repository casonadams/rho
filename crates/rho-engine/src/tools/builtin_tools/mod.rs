pub mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_BASH, PROMPT_EDIT, PROMPT_FD, PROMPT_READ, PROMPT_RG,
    PROMPT_WEB_FETCH, PROMPT_WEB_SEARCH, PROMPT_WRITE,
};

use crate::tools::bash::BashTool;
use crate::tools::edit::EditTool;
use crate::tools::engine_tool::EngineTool;
use crate::tools::fd::FdTool;
use crate::tools::read::ReadTool;
use crate::tools::rg::RgTool;
use crate::tools::subagent::{SubagentModelResolver, SubagentTool};
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::WriteTool;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use std::path::Path;
use std::sync::Arc;

fn build_workspace_native_tools(base_dir: &Path) -> Vec<Arc<dyn EngineTool>> {
    let write = Arc::new(WriteTool::new(base_dir));
    let edit = Arc::new(EditTool::new(base_dir));
    let artifact_dir = base_dir.join(".rho/artifacts");
    let read = Arc::new(ReadTool::new(base_dir).with_artifact_dir(Some(artifact_dir.clone())));
    let bash = Arc::new(BashTool::new(base_dir));
    let fd = Arc::new(FdTool::new(base_dir).with_artifact_dir(Some(artifact_dir.clone())));
    let rg = Arc::new(RgTool::new(base_dir).with_artifact_dir(Some(artifact_dir)));

    vec![
        read as Arc<dyn EngineTool>,
        write as Arc<dyn EngineTool>,
        edit as Arc<dyn EngineTool>,
        bash as Arc<dyn EngineTool>,
        fd as Arc<dyn EngineTool>,
        rg as Arc<dyn EngineTool>,
    ]
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
    build_builtin_toolset(base_dir, config, model, model_resolver, None).map(|(tools, _)| tools)
}

/// The built-in tools and the subagent tool. With a worktree `store`, every subagent works in its own
/// worktree and the `worktree` tool lets the caller review and merge them.
pub fn build_builtin_toolset(
    base_dir: &Path,
    config: &Config,
    model: Option<crate::engine::compactor::llm::ModelHandle>,
    model_resolver: Option<SubagentModelResolver>,
    worktrees: Option<crate::worktree::WorktreeStore>,
) -> Result<(Vec<Arc<dyn EngineTool>>, SubagentTool)> {
    let mut tools = build_workspace_native_tools(base_dir);
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
    let resolved_agents = rho_harness_core::agents::resolved_agents(Some(base_dir));
    let agent_metas: Vec<_> = resolved_agents.into_iter().map(|a| a.metadata).collect();
    let mut subagent = crate::tools::subagent::SubagentTool::new(model, tools.clone())
        .with_agents(agent_metas)
        .with_personas(rho_harness_core::agents::resolved_personas(Some(base_dir)))
        .with_pinned_models(config.subagents.models.clone())
        .with_foreground_budget(std::time::Duration::from_secs(
            config
                .subagents
                .foreground_seconds
                .unwrap_or(crate::tools::subagent::DEFAULT_FOREGROUND_SECONDS),
        ));
    if let Some(resolver) = model_resolver {
        subagent = subagent
            .with_model_resolver(resolver)
            .with_model_roles(config.models.get("smol").cloned(), config.models.get("slow").cloned());
    }
    if let Some(store) = worktrees {
        subagent = subagent.with_worktrees(store.clone());
        tools.push(Arc::new(crate::tools::WorktreeTool::new(base_dir, store)) as Arc<dyn EngineTool>);
    }
    tools.push(Arc::new(subagent.clone()) as Arc<dyn EngineTool>);
    Ok((tools, subagent))
}

/// `tools` with the workspace-native ones (read, write, edit, bash, fd, rg) rooted at `root` instead.
pub(crate) fn rerooted(tools: &[Arc<dyn EngineTool>], root: &Path) -> Vec<Arc<dyn EngineTool>> {
    let native = build_workspace_native_tools(root);
    tools
        .iter()
        .map(|tool| {
            native
                .iter()
                .find(|n| n.name() == tool.name())
                .cloned()
                .unwrap_or_else(|| tool.clone())
        })
        .collect()
}
