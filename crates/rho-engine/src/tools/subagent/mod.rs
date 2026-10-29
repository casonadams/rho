pub mod runner;
#[cfg(test)]
mod tests;
pub mod types;

pub use runner::SubagentRunner;
pub use types::{
    DEFAULT_SUBAGENT_MAX_TURNS, MAX_SUBAGENT_DEPTH, MAX_SUBAGENT_TURNS, SubagentArgs, SubagentRole, clamp_max_turns,
};

use crate::engine::compactor::llm::ModelHandle;
use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
use rho_harness_core::agents::AgentMetadata;
use rho_harness_core::error::AppError;
use std::sync::Arc;

pub struct SubagentTool {
    model: Option<ModelHandle>,
    depth: usize,
    tools: Vec<Arc<dyn EngineTool>>,
    agents: Vec<AgentMetadata>,
    description: String,
}

impl SubagentTool {
    #[must_use]
    pub fn new(model: Option<ModelHandle>, depth: usize, tools: Vec<Arc<dyn EngineTool>>) -> Self {
        Self {
            model,
            depth,
            tools,
            agents: Vec::new(),
            description: "Spawn an isolated subagent with a specialized role ('scout', 'critic', 'planner', 'general') to complete a focused task.".to_string(),
        }
    }

    #[must_use]
    pub fn with_model(mut self, model: ModelHandle) -> Self {
        self.model = Some(model);
        self
    }

    #[must_use]
    pub fn with_agents(mut self, agents: Vec<AgentMetadata>) -> Self {
        if !agents.is_empty() {
            let names = agents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
            self.description = format!(
                "Spawn an isolated subagent with a specialized role ('scout', 'critic', 'planner', 'general') or custom agent ({names}) to complete a focused task."
            );
        }
        self.agents = agents;
        self
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }
}

fn filter_tools_for_agent(tools: &[Arc<dyn EngineTool>], agent: &AgentMetadata) -> Vec<Arc<dyn EngineTool>> {
    let mut filtered: Vec<Arc<dyn EngineTool>> = if let Some(allowed) = &agent.tools {
        tools
            .iter()
            .filter(|t| allowed.iter().any(|a| a.eq_ignore_ascii_case(t.name())))
            .cloned()
            .collect()
    } else {
        tools.to_vec()
    };
    if let Some(disallowed) = &agent.disallowed_tools {
        filtered.retain(|t| !disallowed.iter().any(|d| d.eq_ignore_ascii_case(t.name())));
    }
    filtered.retain(|t| t.name() != "subagent");
    filtered.sort_by(|a, b| a.name().cmp(b.name()));
    filtered
}

#[async_trait::async_trait]
impl EngineTool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<SubagentArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        if self.depth >= MAX_SUBAGENT_DEPTH {
            return Ok(ToolResult::error(
                "Maximum subagent recursion depth (1) reached: child subagents cannot spawn further subagents.",
            ));
        }

        let (role, agent_name, prompt, context_slice, max_turns) =
            match serde_json::from_value::<SubagentArgs>(args.clone()) {
                Ok(parsed) => (
                    parsed.role,
                    parsed.agent,
                    parsed.prompt,
                    parsed.context_slice,
                    parsed.max_turns,
                ),
                Err(err) => {
                    let role_str = args.get("role").and_then(|v| v.as_str());
                    let agent_str = args.get("agent").and_then(|v| v.as_str());
                    let Some(prompt_str) = args.get("prompt").and_then(|v| v.as_str()) else {
                        return Ok(ToolResult::error(format!("failed to parse subagent arguments: {err}")));
                    };
                    let slice = args.get("context_slice").and_then(|v| v.as_str()).map(str::to_string);
                    let turns = args.get("max_turns").and_then(|v| v.as_u64()).map(|u| u as usize);
                    (
                        None,
                        agent_str.or(role_str).map(str::to_string),
                        prompt_str.to_string(),
                        slice,
                        turns,
                    )
                }
            };

        if prompt.trim().is_empty() {
            return Ok(ToolResult::error("subagent prompt cannot be empty"));
        }

        let Some(model) = &self.model else {
            return Ok(ToolResult::error("No model handle configured for subagent execution."));
        };

        if let Some(name) = agent_name.as_deref()
            && let Some(agent) = self.agents.iter().find(|a| a.name.eq_ignore_ascii_case(name))
        {
            let filtered_tools = filter_tools_for_agent(&self.tools, agent);
            let turns = clamp_max_turns(max_turns.or(agent.max_turns));
            let runner = SubagentRunner::new_custom(model.clone(), agent.system_prompt.clone(), filtered_tools, turns);
            let result = runner.run(&prompt, context_slice.as_deref()).await;
            return Ok(result);
        }

        let role = role.unwrap_or_default();
        let max_turns = clamp_max_turns(max_turns);
        let runner = SubagentRunner::new(model.clone(), role, &self.tools, max_turns);

        let result = runner.run(&prompt, context_slice.as_deref()).await;
        Ok(result)
    }
}
