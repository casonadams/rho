pub mod discovery;
pub mod runner;
pub mod supervisor;
pub mod tools;
pub mod types;

use std::path::Path;
use std::sync::Arc;

pub use discovery::{builtin_templates, discover_templates};
pub use runner::{NoopProvider, SubagentRunner};
pub use supervisor::SubagentSupervisor;
pub use tools::{AgentTool, GetSubagentResultTool, SteerSubagentTool, create_subagent_tools};
pub use types::{AgentExecutionResult, AgentInvocationArgs, AgentTemplate};

pub fn load_subagent_capabilities(
    config: &rho_core::config::Config,
    base_dir: &Path,
    provider: Option<Arc<dyn rho_sdk::contract::ProviderCapability>>,
) -> Vec<(
    rho_sdk::capability::CapabilityId,
    Arc<dyn rho_sdk::contract::ToolCapability>,
)> {
    if !config.subagents.enabled {
        return Vec::new();
    }
    let provider = provider.unwrap_or_else(|| Arc::new(NoopProvider) as Arc<dyn rho_sdk::contract::ProviderCapability>);
    let runner = Arc::new(SubagentRunner::new(provider, config.subagents.max_turns_per_agent));
    let supervisor = SubagentSupervisor::new(runner, config.subagents.max_concurrency);
    create_subagent_tools(supervisor, config, base_dir)
}
