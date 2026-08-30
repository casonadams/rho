//! Host platform assembly: the application loads the active tool platform
//! (built-in tools + configured plugins + MCP) and hands the prepared forms
//! to the engine.

use rho_core::config::Config;
use rho_core::error::Result;
use rho_engine::auth::AuthStore;
use rho_engine::engine::{AgentEngine, builder::AgentEngineBuilder};
use rho_host::tool_dispatch::ActiveToolSet;
use rho_sdk::contract::{CommandCapability, ContextCapability, LifecycleCapability};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub struct ToolAssembly {
    pub rig_tools: Vec<rig::tool::DynamicTool>,
    pub neutral_executor: Arc<dyn rho_core::dispatch::NeutralToolExecutor>,
    pub contexts: Vec<Arc<dyn ContextCapability>>,
    pub commands: BTreeMap<String, Arc<dyn CommandCapability>>,
    pub lifecycles: Vec<Arc<dyn LifecycleCapability>>,
}

/// Assemble the active tool platform for a config, including configured
/// external plugins and MCP servers.
pub async fn active_tools(config: &Config, base_dir: &Path) -> Result<ToolAssembly> {
    let tool_set = std::sync::Arc::new(ActiveToolSet::load(config, base_dir).await?);
    let neutral_executor = tool_set.neutral_executor(rig::tool::ToolContext::default());
    let rig_tools = ActiveToolSet::clone(&tool_set).into_rig_tools();
    let contexts = tool_set.active_contexts();
    let commands = tool_set.active_commands();
    let lifecycles = tool_set.active_lifecycles();
    Ok(ToolAssembly {
        rig_tools,
        neutral_executor: std::sync::Arc::new(neutral_executor),
        contexts,
        commands,
        lifecycles,
    })
}

impl ToolAssembly {
    pub fn into_parts(
        self,
    ) -> (
        Vec<rig::tool::DynamicTool>,
        Arc<dyn rho_core::dispatch::NeutralToolExecutor>,
    ) {
        (self.rig_tools, self.neutral_executor)
    }
}

/// Build the interactive application engine with the platform injected.
pub async fn agent_engine(config: Config, auth_store: AuthStore, resume: Option<&str>) -> Result<AgentEngine> {
    let base_dir = std::env::current_dir()?;
    let assembly = active_tools(&config, &base_dir).await?;
    AgentEngineBuilder::new(config, auth_store)
        .resume(resume)
        .base_dir(base_dir)
        .contexts(assembly.contexts)
        .lifecycles(assembly.lifecycles)
        .tool_assembly(assembly.rig_tools, assembly.neutral_executor)
        .build()
        .await
}
