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
use rho_harness_core::error::AppError;
use rig::tool::DynamicTool;
use std::sync::Arc;

pub struct SubagentTool {
    model: Option<ModelHandle>,
    depth: usize,
    tools: Vec<DynamicTool>,
}

impl SubagentTool {
    #[must_use]
    pub fn new(model: Option<ModelHandle>, depth: usize, tools: Vec<DynamicTool>) -> Self {
        Self { model, depth, tools }
    }

    #[must_use]
    pub fn with_model(mut self, model: ModelHandle) -> Self {
        self.model = Some(model);
        self
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }
}

#[async_trait::async_trait]
impl EngineTool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }

    fn description(&self) -> &str {
        "Spawn an isolated subagent with a specialized role ('scout', 'critic', 'planner', 'general') to complete a focused task."
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

        let parsed: SubagentArgs = match serde_json::from_value(args) {
            Ok(args) => args,
            Err(err) => return Ok(ToolResult::error(format!("failed to parse subagent arguments: {err}"))),
        };

        let Some(model) = &self.model else {
            return Ok(ToolResult::error("No model handle configured for subagent execution."));
        };

        let role = parsed.role.unwrap_or_default();
        let max_turns = clamp_max_turns(parsed.max_turns);
        let runner = SubagentRunner::new(model.clone(), role, &self.tools, max_turns);

        let result = runner.run(&parsed.prompt, parsed.context_slice.as_deref()).await;
        Ok(result)
    }
}

#[must_use]
pub fn make_subagent_tool(tool: Arc<SubagentTool>) -> DynamicTool {
    crate::adapter::rig::tools::into_dynamic_tool_arc(tool)
}
