pub mod discovery;
pub mod runner;
pub mod supervisor;
pub mod tools;
pub mod types;

use std::path::Path;
use std::sync::Arc;

pub use discovery::{builtin_templates, discover_templates, parse_agent_markdown};
pub use runner::{
    NoopExecutor, NoopProvider, SubagentExecuteRequest, SubagentExecutor, SubagentRunner, resolve_subagent_model,
};
pub use supervisor::SubagentSupervisor;
pub use tools::{AgentTool, GetSubagentResultTool, SteerSubagentTool, create_subagent_tools};
pub use types::{AgentExecutionResult, AgentInvocationArgs, AgentTemplate};

pub fn load_subagent_capabilities(
    config: &rho_core::config::Config,
    base_dir: &Path,
    executor: Option<Arc<dyn SubagentExecutor>>,
) -> Vec<(
    rho_sdk::capability::CapabilityId,
    Arc<dyn rho_sdk::contract::ToolCapability>,
)> {
    if !config.subagents.enabled {
        return Vec::new();
    }
    let executor = executor.unwrap_or_else(|| Arc::new(NoopExecutor) as Arc<dyn SubagentExecutor>);
    let runner = Arc::new(SubagentRunner::new(executor, config.subagents.max_turns_per_agent));
    let supervisor = SubagentSupervisor::new(runner, config.subagents.max_concurrency);
    create_subagent_tools(supervisor, config, base_dir)
}
