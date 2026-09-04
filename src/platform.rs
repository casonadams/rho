//! Host platform assembly: loads built-in tools and configured MCP servers.

pub mod clipboard;
pub mod suspend;

use rho_engine::auth::AuthStore;
use rho_engine::engine::{AgentEngine, builder::AgentEngineBuilder};
use rho_engine::mcp::load_mcp_tools;
use rho_engine::tools::build_builtin_tools;
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use std::path::Path;

pub struct ToolAssembly {
    pub rig_tools: Vec<rig::tool::DynamicTool>,
}

pub async fn active_tools(config: &Config, base_dir: &Path) -> Result<ToolAssembly> {
    let mut tools = build_builtin_tools(base_dir, config)?;
    let mcp_tools = load_mcp_tools(config, base_dir).await;
    tools.extend(mcp_tools);
    Ok(ToolAssembly { rig_tools: tools })
}

pub async fn active_tools_with_auth(config: &Config, base_dir: &Path, _auth_store: &AuthStore) -> Result<ToolAssembly> {
    active_tools(config, base_dir).await
}

pub async fn agent_engine(config: Config, auth_store: AuthStore, resume: Option<&str>) -> Result<AgentEngine> {
    let base_dir = std::env::current_dir()?;
    AgentEngineBuilder::new(config, auth_store)
        .resume(resume)
        .base_dir(base_dir)
        .build()
        .await
}
