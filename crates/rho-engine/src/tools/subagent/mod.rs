pub(crate) mod conversation;
pub(crate) mod evidence;
pub(crate) mod handoff;
mod inspect;
mod recovery;
pub mod runner;
mod snapshot;
#[cfg(test)]
mod tests;
pub mod types;
pub(crate) mod workspace;

pub use runner::SubagentRunner;
pub use types::{DEFAULT_SUBAGENT_TURNS, SubagentArgs, SubagentRole, resolve_turns, task_name};

use crate::engine::compactor::llm::ModelHandle;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::{ToolResult, generated_schema};
use rho_harness_core::agents::AgentMetadata;
use rho_harness_core::error::AppError;
use rho_harness_core::workflow::ModelRole;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Resolves a configured `<provider>/<model>` name to a handle; the error text is shown to the caller.
pub type SubagentModelResolver = Arc<dyn Fn(&str) -> Result<ModelHandle, String> + Send + Sync>;

#[derive(Clone)]
pub struct SubagentTool {
    model: Option<ModelHandle>,
    tools: Vec<Arc<dyn EngineTool>>,
    agents: Vec<AgentMetadata>,
    description: String,
    ctx: TurnToolContext,
    model_resolver: Option<SubagentModelResolver>,
    roles: BTreeMap<&'static str, String>,
    busy: conversation::BusyContexts,
    worktrees: Option<crate::worktree::WorktreeStore>,
}

#[derive(serde::Deserialize)]
struct RawSubagentArgs {
    #[serde(default)]
    prompt: String,
    task_id: Option<String>,
    inspect: Option<bool>,
    name: Option<String>,
    agent: Option<String>,
    role: Option<String>,
    context_slice: Option<String>,
    model: Option<String>,
    #[serde(alias = "max_turns")]
    turns: Option<usize>,
    inherit_context: Option<bool>,
}

pub(crate) struct RunnerSpec<'a> {
    pub agent: Option<&'a str>,
    pub local_agents: &'a [AgentMetadata],
    pub turns: Option<usize>,
    pub model: Option<ModelRole>,
    pub position: Option<String>,
    pub task_name: Option<String>,
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
            roles: BTreeMap::new(),
            busy: conversation::BusyContexts::default(),
            worktrees: None,
            description: crate::tools::ToolRegistry::builtin_description("subagent").to_string(),
        }
    }

    #[must_use]
    pub fn with_agents(mut self, agents: Vec<AgentMetadata>) -> Self {
        if !agents.is_empty() {
            let names = agents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
            self.description = format!(
                "{}\n- Custom agents (pass the name as `agent`): {names}.",
                crate::tools::ToolRegistry::builtin_description("subagent")
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

    /// Where every subagent's worktree is created; without a store subagents work in the project itself.
    #[must_use]
    pub fn with_worktrees(mut self, store: crate::worktree::WorktreeStore) -> Self {
        self.worktrees = Some(store);
        self
    }

    pub(crate) fn worktrees(&self) -> Option<&crate::worktree::WorktreeStore> {
        self.worktrees.as_ref()
    }

    async fn attach_worktree(&self, exchange: conversation::Exchange) -> Result<conversation::Exchange, String> {
        let label = format!("subagent {}", exchange.settings().name.as_deref().unwrap_or("task"));
        match (&self.worktrees, &self.ctx.project) {
            (Some(store), Some(project)) => exchange.with_worktree(store, &project.current_dir, &label).await,
            _ => Ok(exchange),
        }
    }

    /// Configured model names for the `smol` and `slow` roles; `default` is always the session model.
    #[must_use]
    pub fn with_model_roles(mut self, smol: Option<String>, slow: Option<String>) -> Self {
        let configured = |m: Option<String>| m.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        for (role, model) in [("smol", configured(smol)), ("slow", configured(slow))] {
            if let Some(model) = model {
                self.roles.insert(role, model);
            }
        }
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

    /// Resolves a model role: an explicit role must be configured; no role means `smol`, else the session model.
    async fn model_for(&self, role: Option<ModelRole>, default: &ModelHandle) -> Result<ModelHandle, String> {
        let name = match role {
            Some(ModelRole::Default) => return Ok(default.clone()),
            Some(role) => Some(self.roles.get(role.as_str()).ok_or_else(|| {
                format!(
                    "model role '{}' is not configured (set models.{} in config.toml)",
                    role.as_str(),
                    role.as_str()
                )
            })?),
            None => self.roles.get("smol"),
        };
        let Some(name) = name else {
            return Ok(default.clone());
        };
        let resolver = self
            .model_resolver
            .clone()
            .ok_or_else(|| format!("model '{name}' requested but no model resolver is configured"))?;
        let name = name.clone();
        let label = name.clone();
        tokio::task::spawn_blocking(move || resolver(&name))
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
            .map_err(|e| format!("failed to resolve model '{label}' for subagent: {e}"))
    }

    pub(crate) async fn runner_for(&self, spec: RunnerSpec<'_>) -> Result<SubagentRunner, String> {
        let default_model = self
            .model
            .as_ref()
            .ok_or_else(|| "No model handle configured for subagent execution.".to_string())?;
        let target = self.resolve_target(spec.agent, spec.local_agents)?;
        let model = self.model_for(spec.model, default_model).await?;
        let turns = resolve_turns(spec.turns);
        let runner = match &target {
            Target::Agent(agent) => {
                SubagentRunner::new_custom(model, agent.system_prompt.clone(), self.tools_for(&target), turns)
                    .with_label(&agent.name)
            }
            Target::Role(role) => SubagentRunner::new(model, *role, &self.tools, turns),
        };
        let runner = runner.with_context(self.ctx.clone());
        let runner = match spec.task_name {
            Some(name) => runner.with_task_name(&name),
            None => runner,
        };
        Ok(match spec.position {
            Some(position) => runner.with_position(position),
            None => runner,
        })
    }

    pub(crate) fn turn_context(&self) -> &TurnToolContext {
        &self.ctx
    }

    pub(crate) fn role_configured(&self, role: ModelRole) -> bool {
        role == ModelRole::Default || self.roles.contains_key(role.as_str())
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

    async fn runner_for_settings(
        &self,
        settings: &conversation::ConversationSettings,
        turns: Option<usize>,
    ) -> Result<SubagentRunner, String> {
        let model = settings.model.as_deref().map(ModelRole::parse).transpose()?;
        let local_agents: Vec<AgentMetadata> = settings
            .agent_file
            .as_deref()
            .and_then(|file| rho_harness_core::agents::parse_agent_file(std::path::Path::new(file)))
            .into_iter()
            .collect();
        self.runner_for(RunnerSpec {
            agent: settings.agent.as_deref(),
            local_agents: &local_agents,
            turns,
            model,
            position: None,
            task_name: settings.name.clone(),
        })
        .await
    }

    async fn inspect(&self, task_id: Option<&str>) -> ToolResult {
        let Some(session) = &self.ctx.session else {
            return ToolResult::error("inspecting subagent tasks needs a saved session");
        };
        let Some(task_id) = task_id.filter(|id| !id.is_empty()) else {
            return ToolResult::success(inspect::render_list(&session.subagent_contexts().await));
        };
        let root = workspace::root_of(&self.ctx);
        let rendered = match session.subagent_context(task_id).await {
            Some(context) => inspect::render_task(&context, task_id, &root).await,
            None => None,
        };
        rendered.map_or_else(
            || ToolResult::error(format!("unknown subagent task '{task_id}'")),
            ToolResult::success,
        )
    }

    async fn start_task(&self, args: &RawSubagentArgs) -> Result<ToolResult, String> {
        let settings = conversation::ConversationSettings {
            name: args.name.as_deref().and_then(types::task_name),
            agent: args.agent.clone().or_else(|| args.role.clone()),
            model: args.model.clone(),
            agent_file: None,
        };
        let runner = self.runner_for_settings(&settings, args.turns).await?;
        let context = self.compose_context(args);
        let Some(session) = &self.ctx.session else {
            return Ok(runner.run(&args.prompt, context.as_deref()).await);
        };
        let exchange = conversation::Exchange::start(&settings);
        let _claim = self.busy.claim(exchange.context_id())?;
        let exchange = self.attach_worktree(exchange).await?;
        let input = runner::build_subagent_prompt(&args.prompt, context.as_deref());
        Ok(exchange
            .converse(&runner.with_parent_channel(), session, &input, None)
            .await)
    }

    async fn continue_task(&self, args: &RawSubagentArgs, task_id: &str) -> Result<ToolResult, String> {
        let session = self
            .ctx
            .session
            .as_ref()
            .ok_or("continuing a subagent task needs a saved session")?;
        let context = session
            .subagent_context(task_id)
            .await
            .ok_or_else(|| format!("unknown subagent task '{task_id}'"))?;
        let exchange = conversation::Exchange::resume(context, task_id)?;
        let mut settings = exchange.settings();
        let requested = args.agent.as_deref().or(args.role.as_deref());
        if let Some(requested) =
            requested.filter(|r| !settings.agent.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(r)))
        {
            return Err(format!(
                "task '{task_id}' runs as '{}', not '{requested}'; omit role and agent when continuing a task",
                settings.agent.as_deref().unwrap_or("scout")
            ));
        }
        settings.model = args.model.clone().or(settings.model);
        let runner = self.runner_for_settings(&settings, args.turns).await?;
        let _claim = self.busy.claim(exchange.context_id())?;
        let exchange = self.attach_worktree(exchange).await?;
        let input = runner::build_subagent_prompt(&args.prompt, self.compose_context(args).as_deref());
        Ok(exchange
            .converse(&runner.with_parent_channel(), session, &input, Some(task_id))
            .await)
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
        let args: RawSubagentArgs = match crate::tools::types::parse_tool_args("subagent", args, || self.parameters()) {
            Ok(parsed) => parsed,
            Err(e) => return Ok(e),
        };
        if args.inspect.unwrap_or(false) {
            return Ok(self.inspect(args.task_id.as_deref().map(str::trim)).await);
        }
        if args.prompt.trim().is_empty() {
            return Ok(ToolResult::error("subagent prompt cannot be empty"));
        }
        let result = match args.task_id.as_deref().map(str::trim) {
            Some(task_id) if !task_id.is_empty() => self.continue_task(&args, task_id).await,
            _ => self.start_task(&args).await,
        };
        Ok(result.unwrap_or_else(ToolResult::error))
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(self.with_turn_context(ctx)))
    }

    fn concurrency_safe(&self, args: &serde_json::Value) -> bool {
        let args = crate::tools::types::normalize_tool_arguments(args.clone());
        let Ok(parsed) = crate::tools::types::try_parse_tool_args::<RawSubagentArgs>(&args) else {
            return false;
        };
        if parsed.inspect.unwrap_or(false) {
            return true;
        }
        if parsed.task_id.is_some() {
            return false;
        }
        let Ok(target) = self.resolve_target(parsed.agent.as_deref().or(parsed.role.as_deref()), &[]) else {
            return false;
        };
        let tools = self.tools_for(&target);
        !tools.is_empty() && tools.iter().all(|t| runner::READ_ONLY_TOOLS.contains(&t.name()))
    }
}
