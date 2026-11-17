mod recovery;
pub mod runner;
#[cfg(test)]
mod tests;
pub mod types;

pub use runner::SubagentRunner;
pub use types::{DEFAULT_SUBAGENT_MAX_TURNS, MAX_SUBAGENT_TURNS, SubagentArgs, SubagentRole, clamp_max_turns};

use crate::engine::compactor::llm::ModelHandle;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::{ToolResult, generated_schema};
use rho_harness_core::agents::AgentMetadata;
use rho_harness_core::error::AppError;
use std::sync::Arc;

/// Resolves a custom agent's `model` field to a handle; the error text is shown to the calling model.
pub type SubagentModelResolver = Arc<dyn Fn(&str) -> Result<ModelHandle, String> + Send + Sync>;

const INHERIT_MODEL_NAMES: [&str; 4] = ["inherit", "sonnet", "opus", "haiku"];

#[derive(Clone)]
pub struct SubagentTool {
    model: Option<ModelHandle>,
    tools: Vec<Arc<dyn EngineTool>>,
    agents: Vec<AgentMetadata>,
    description: String,
    ctx: TurnToolContext,
    model_resolver: Option<SubagentModelResolver>,
    default_model: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawSubagentArgs {
    prompt: String,
    agent: Option<String>,
    role: Option<String>,
    context_slice: Option<String>,
    max_turns: Option<usize>,
    inherit_context: Option<bool>,
}

pub(crate) struct RunnerSpec<'a> {
    pub agent: Option<&'a str>,
    pub local_agents: &'a [AgentMetadata],
    pub max_turns: Option<usize>,
    pub model: Option<&'a str>,
}

enum Target<'a> {
    Agent(&'a AgentMetadata),
    Role(SubagentRole),
}

impl SubagentTool {
    #[must_use]
    pub fn new(model: Option<ModelHandle>, tools: Vec<Arc<dyn EngineTool>>) -> Self {
        Self {
            model,
            tools,
            agents: Vec::new(),
            ctx: TurnToolContext::default(),
            model_resolver: None,
            default_model: None,
            description: "Spawn an isolated subagent with a specialized role ('scout', 'critic', 'planner', 'general') to complete a focused task.".to_string(),
        }
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
    pub fn with_model_resolver(mut self, resolver: SubagentModelResolver) -> Self {
        self.model_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn with_default_model(mut self, model: Option<String>) -> Self {
        self.default_model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        self
    }

    fn resolve_target<'a>(&'a self, name: Option<&str>, local: &'a [AgentMetadata]) -> Result<Target<'a>, String> {
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(Target::Role(SubagentRole::default()));
        };
        let find = |agents: &'a [AgentMetadata]| agents.iter().find(|a| a.name.eq_ignore_ascii_case(name));
        if let Some(agent) = find(local).or_else(|| find(&self.agents)) {
            return Ok(Target::Agent(agent));
        }
        name.parse::<SubagentRole>()
            .map(Target::Role)
            .map_err(|_| self.unknown_target(name))
    }

    fn tools_for(&self, target: &Target<'_>) -> Vec<Arc<dyn EngineTool>> {
        match target {
            Target::Agent(agent) => filter_tools_for_agent(&self.tools, agent),
            Target::Role(role) => runner::filter_tools_for_role(&self.tools, *role),
        }
    }

    fn model_for(
        &self,
        target: &Target<'_>,
        override_model: Option<&str>,
        default: &ModelHandle,
    ) -> Result<ModelHandle, String> {
        let agent_model = match target {
            Target::Agent(agent) => agent.model.as_deref(),
            Target::Role(_) => None,
        };
        let requested = override_model
            .or(agent_model)
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .or(self.default_model.as_deref());
        let Some(requested) = requested else {
            return Ok(default.clone());
        };
        if INHERIT_MODEL_NAMES.iter().any(|n| n.eq_ignore_ascii_case(requested)) {
            return Ok(default.clone());
        }
        let resolver = self
            .model_resolver
            .as_ref()
            .ok_or_else(|| format!("agent requests model '{requested}' but no model resolver is configured"))?;
        resolver(requested).map_err(|e| format!("failed to resolve model '{requested}' for subagent: {e}"))
    }

    pub(crate) fn runner_for(&self, spec: RunnerSpec<'_>) -> Result<SubagentRunner, String> {
        let default_model = self
            .model
            .as_ref()
            .ok_or_else(|| "No model handle configured for subagent execution.".to_string())?;
        let target = self.resolve_target(spec.agent, spec.local_agents)?;
        let model = self.model_for(&target, spec.model, default_model)?;
        let runner = match &target {
            Target::Agent(agent) => SubagentRunner::new_custom(
                model,
                agent.system_prompt.clone(),
                self.tools_for(&target),
                clamp_max_turns(spec.max_turns.or(agent.max_turns)),
            )
            .with_label(&agent.name),
            Target::Role(role) => SubagentRunner::new(model, *role, &self.tools, clamp_max_turns(spec.max_turns)),
        };
        Ok(runner.with_context(self.ctx.clone()))
    }

    pub(crate) fn turn_context(&self) -> &TurnToolContext {
        &self.ctx
    }

    pub(crate) fn model(&self) -> Option<&ModelHandle> {
        self.model.as_ref()
    }

    pub(crate) fn with_turn_context(&self, ctx: &TurnToolContext) -> Self {
        Self {
            ctx: ctx.clone(),
            ..self.clone()
        }
    }

    fn compose_context(&self, args: &RawSubagentArgs) -> Option<String> {
        let parent = args
            .inherit_context
            .unwrap_or(false)
            .then(|| runner::render_parent_conversation(&self.ctx.parent_history))
            .filter(|text| !text.is_empty());
        match (parent, args.context_slice.as_deref().filter(|s| !s.trim().is_empty())) {
            (Some(parent), Some(slice)) => Some(format!("{parent}\n\n{slice}")),
            (Some(text), None) => Some(text),
            (None, slice) => slice.map(str::to_string),
        }
    }

    fn unknown_target(&self, name: &str) -> String {
        let mut valid: Vec<&str> = vec!["scout", "critic", "planner", "general"];
        valid.extend(self.agents.iter().map(|a| a.name.as_str()));
        format!("unknown subagent '{name}'; valid: {}", valid.join(", "))
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
        let args = match serde_json::from_value::<RawSubagentArgs>(args) {
            Ok(parsed) => parsed,
            Err(err) => return Ok(ToolResult::error(format!("failed to parse subagent arguments: {err}"))),
        };
        if args.prompt.trim().is_empty() {
            return Ok(ToolResult::error("subagent prompt cannot be empty"));
        }
        let spec = RunnerSpec {
            agent: args.agent.as_deref().or(args.role.as_deref()),
            local_agents: &[],
            max_turns: args.max_turns,
            model: None,
        };
        match self.runner_for(spec) {
            Ok(runner) => Ok(runner.run(&args.prompt, self.compose_context(&args).as_deref()).await),
            Err(message) => Ok(ToolResult::error(message)),
        }
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(self.with_turn_context(ctx)))
    }

    fn concurrency_safe(&self, args: &serde_json::Value) -> bool {
        let Ok(parsed) = serde_json::from_value::<RawSubagentArgs>(args.clone()) else {
            return false;
        };
        let Ok(target) = self.resolve_target(parsed.agent.as_deref().or(parsed.role.as_deref()), &[]) else {
            return false;
        };
        let tools = self.tools_for(&target);
        !tools.is_empty() && tools.iter().all(|t| runner::READ_ONLY_TOOLS.contains(&t.name()))
    }
}
