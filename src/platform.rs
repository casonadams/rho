//! Host platform assembly: loads built-in tools and configured MCP servers.

pub mod clipboard;
pub mod git;
#[cfg(feature = "ui")]
pub mod suspend;
pub mod terminal_session;

use rho_engine::auth::AuthStore;
use rho_engine::engine::AgentEngine;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_engine::provider::ModelCatalog;
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

pub fn agent_engine_in_dir_with_presenter(
    config: Config,
    auth_store: AuthStore,
    catalog: ModelCatalog,
    base_dir: PathBuf,
    resume: Option<&str>,
    presenter: Option<std::sync::Arc<dyn rho_harness_core::presentation::Presenter>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AgentEngine>> + Send + '_>> {
    Box::pin(async move {
        let mut builder = AgentEngineBuilder::new(config, auth_store)
            .model_catalog(catalog)
            .resume(resume)
            .base_dir(base_dir);
        if let Some(presenter) = presenter {
            builder = builder.presenter(presenter);
        }
        let engine = builder.build().await?;
        // A hint for external tools must never stop a session from starting.
        let _ =
            terminal_session::record_terminal_session(&engine.config.config_dir, &engine.session_manager.session_id);
        Ok(engine)
    })
}

pub async fn agent_engine_in_dir(
    config: Config,
    auth_store: AuthStore,
    catalog: ModelCatalog,
    base_dir: PathBuf,
    resume: Option<&str>,
) -> Result<AgentEngine> {
    agent_engine_in_dir_with_presenter(config, auth_store, catalog, base_dir, resume, None).await
}

pub async fn agent_engine_with_presenter(
    config: Config,
    auth_store: AuthStore,
    catalog: ModelCatalog,
    resume: Option<&str>,
    presenter: Option<std::sync::Arc<dyn rho_harness_core::presentation::Presenter>>,
) -> Result<AgentEngine> {
    let base_dir = std::env::current_dir()?;
    agent_engine_in_dir_with_presenter(config, auth_store, catalog, base_dir, resume, presenter).await
}

pub async fn agent_engine(
    config: Config,
    auth_store: AuthStore,
    catalog: ModelCatalog,
    resume: Option<&str>,
) -> Result<AgentEngine> {
    agent_engine_with_presenter(config, auth_store, catalog, resume, None).await
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
                    "undo",
                    "bash",
                    "fd",
                    "rg",
                    "web_search",
                    "web_fetch",
                    "todo",
                    "wait",
                    "memory",
                    "subagent",
                ],
            ),
            (
                false,
                true,
                vec![
                    "read",
                    "write",
                    "edit",
                    "undo",
                    "bash",
                    "fd",
                    "rg",
                    "web_fetch",
                    "todo",
                    "wait",
                    "memory",
                    "subagent",
                ],
            ),
            (
                true,
                false,
                vec![
                    "read",
                    "write",
                    "edit",
                    "undo",
                    "bash",
                    "fd",
                    "rg",
                    "web_search",
                    "todo",
                    "wait",
                    "memory",
                    "subagent",
                ],
            ),
            (
                false,
                false,
                vec![
                    "read", "write", "edit", "undo", "bash", "fd", "rg", "todo", "wait", "memory", "subagent",
                ],
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
