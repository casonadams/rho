//! Host platform assembly: loads built-in tools and configured MCP servers.

pub mod clipboard;
pub mod suspend;

use rho_engine::auth::AuthStore;
use rho_engine::engine::AgentEngine;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use std::path::{Path, PathBuf};

pub struct ToolAssembly {
    pub tools: Vec<std::sync::Arc<dyn rho_engine::tools::EngineTool>>,
}

pub async fn active_tools(config: &Config, base_dir: &Path) -> Result<ToolAssembly> {
    let tools = rho_engine::tools::builtin_tools::build_builtin_tools(base_dir, config, None)?;
    Ok(ToolAssembly { tools })
}

pub async fn active_tools_with_auth(
    config: &Config,
    base_dir: &Path,
    _auth_store: &rho_engine::auth::AuthStore,
) -> Result<ToolAssembly> {
    active_tools(config, base_dir).await
}

pub async fn agent_engine_in_dir(
    config: Config,
    auth_store: AuthStore,
    base_dir: PathBuf,
    resume: Option<&str>,
) -> Result<AgentEngine> {
    AgentEngineBuilder::new(config, auth_store)
        .resume(resume)
        .base_dir(base_dir)
        .build()
        .await
}

pub async fn agent_engine(config: Config, auth_store: AuthStore, resume: Option<&str>) -> Result<AgentEngine> {
    let base_dir = std::env::current_dir()?;
    agent_engine_in_dir(config, auth_store, base_dir, resume).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn active_tools_follow_web_tool_toggles() {
        let base_dir = tempfile::tempdir().unwrap();
        let cases = [
            (
                true,
                true,
                vec![
                    "read",
                    "write",
                    "edit",
                    "bash",
                    "fd",
                    "rg",
                    "web_search",
                    "web_fetch",
                    "subagent",
                ],
            ),
            (
                false,
                true,
                vec!["read", "write", "edit", "bash", "fd", "rg", "web_fetch", "subagent"],
            ),
            (
                true,
                false,
                vec!["read", "write", "edit", "bash", "fd", "rg", "web_search", "subagent"],
            ),
            (
                false,
                false,
                vec!["read", "write", "edit", "bash", "fd", "rg", "subagent"],
            ),
        ];
        for (search_enabled, fetch_enabled, expected) in cases {
            let mut config = Config::default();
            config.tools.web.search.enabled = search_enabled;
            config.tools.web.fetch.enabled = fetch_enabled;
            let assembly = active_tools(&config, base_dir.path()).await.unwrap();
            let names: Vec<&str> = assembly.tools.iter().map(|tool| tool.name()).collect();
            assert_eq!(names, expected, "search={search_enabled} fetch={fetch_enabled}");
        }
    }
}
