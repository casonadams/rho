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
use crate::tools::subagent::SubagentModelResolver;
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
    let mut subagent = crate::tools::subagent::SubagentTool::new(model, tools.clone()).with_agents(agent_metas);
    if let Some(resolver) = model_resolver {
        subagent = subagent.with_model_resolver(resolver);
    }
    tools.push(Arc::new(subagent) as Arc<dyn EngineTool>);
    Ok(tools)
}
