pub mod agent;
pub mod control;
#[cfg(test)]
mod tests;

pub use agent::{AgentTool, PROMPT_AGENT};
pub use control::{GetSubagentResultArgs, GetSubagentResultTool, SteerSubagentArgs, SteerSubagentTool};

use super::supervisor::SubagentSupervisor;
use rho_core::config::Config;
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::ToolCapability;
use std::path::Path;
use std::sync::Arc;

pub fn create_subagent_tools(
    supervisor: SubagentSupervisor,
    config: &Config,
    workspace_dir: &Path,
) -> Vec<(CapabilityId, Arc<dyn ToolCapability>)> {
    if !config.subagents.enabled {
        return Vec::new();
    }

    let agent_tool = Arc::new(AgentTool::new(supervisor.clone(), config.clone(), workspace_dir));
    let get_result_tool = Arc::new(GetSubagentResultTool::new(supervisor.clone()));
    let steer_tool = Arc::new(SteerSubagentTool::new(supervisor));

    vec![
        (agent_tool.descriptor().id, agent_tool),
        (get_result_tool.descriptor().id, get_result_tool),
        (steer_tool.descriptor().id, steer_tool),
    ]
}
