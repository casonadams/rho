mod background;
pub(crate) mod conversation;
pub(crate) mod evidence;
pub(crate) mod handoff;
mod inspect;
mod models;
mod observed;
mod presenter;
mod recovery;
pub mod round;
pub mod runner;
mod snapshot;
mod tasks;
#[cfg(test)]
mod tests;
pub mod types;
pub(crate) mod workspace;

pub(crate) use background::RoundSlot;
pub use background::{DEFAULT_FOREGROUND_SECONDS, SubagentRuntime};
pub use models::{SubagentModelResolver, SubagentModels};
pub use runner::SubagentRunner;
pub use types::{DEFAULT_SUBAGENT_TURNS, SubagentArgs, SubagentRole, resolve_turns, task_name};

use crate::engine::compactor::llm::ModelHandle;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::{ToolResult, generated_schema};
use rho_harness_core::agents::{AgentMetadata, PersonaMetadata};
use rho_harness_core::error::AppError;
use rho_harness_core::workflow::ModelRole;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct SubagentTool {
    models: SubagentModels,
    tools: Vec<Arc<dyn EngineTool>>,
    agents: Vec<AgentMetadata>,
    personas: Vec<PersonaMetadata>,
    description: String,
    ctx: TurnToolContext,
    roles: BTreeMap<ModelRole, String>,
    pinned_models: BTreeMap<String, ModelRole>,
    busy: conversation::BusyContexts,
    worktrees: Option<crate::worktree::WorktreeStore>,
    background: background::Backgrounding,
    /// Recent conversation a subagent keeps word for word when it compacts.
    keep_recent_tokens: usize,
}

#[derive(serde::Deserialize)]
struct RawSubagentArgs {
    #[serde(default)]
    prompt: String,
    task_id: Option<String>,
    inspect: Option<bool>,
    name: Option<String>,
    agent: Option<String>,
    persona: Option<String>,
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
    pub persona: Option<&'a str>,
}

enum Target<'a> {
    Agent(&'a AgentMetadata),
    Role(SubagentRole),
}

impl SubagentTool {
    #[must_use]
    pub fn new(model: Option<ModelHandle>, tools: Vec<Arc<dyn EngineTool>>) -> Self {
        Self {
            models: SubagentModels::new(model),
            tools,
            agents: Vec::new(),
            personas: Vec::new(),
            ctx: TurnToolContext::default(),
            roles: BTreeMap::new(),
            pinned_models: BTreeMap::new(),
            busy: conversation::BusyContexts::default(),
            worktrees: None,
            background: background::Backgrounding::default(),
            keep_recent_tokens: rho_harness_core::tokens::DEFAULT_KEEP_RECENT_TOKENS,
            description: String::new(),
        }
        .describe()
    }

    /// How much recent conversation a subagent keeps word for word when it compacts.
    #[must_use]
    pub fn with_keep_recent_tokens(mut self, tokens: usize) -> Self {
        self.keep_recent_tokens = tokens;
        self
    }

    #[must_use]
    pub fn with_agents(mut self, agents: Vec<AgentMetadata>) -> Self {
        self.agents = agents;
        self.describe()
    }

    /// Personas the caller can layer onto any role or agent with `persona`.
    #[must_use]
    pub fn with_personas(mut self, personas: Vec<PersonaMetadata>) -> Self {
        self.personas = personas;
        self.describe()
    }

    fn describe(mut self) -> Self {
        let mut description = crate::tools::ToolRegistry::builtin_description("subagent").to_string();
        if !self.agents.is_empty() {
            let names = self
                .agents
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            description.push_str(&format!("\n- Custom agents (pass the name as `agent`): {names}."));
        }
        if !self.personas.is_empty() {
            let listed: Vec<String> = self
                .personas
                .iter()
                .map(|p| match p.description.trim() {
                    "" => p.name.clone(),
                    description => format!("{} ({description})", p.name),
                })
                .collect();
            description.push_str(&format!(
                "\n- Personas (pass the name as `persona` to add its behavior instructions to any role or agent; tools stay the role's or agent's): {}.",
                listed.join(", ")
            ));
        }
        self.description = description;
        self
    }

    #[must_use]
    pub fn with_model_resolver(self, resolver: SubagentModelResolver) -> Self {
        self.models.set_resolver(resolver);
        self
    }

    /// The models every copy of this tool runs subagents on.
    pub fn models(&self) -> SubagentModels {
        self.models.clone()
    }

    /// Where every subagent's worktree is created; without a store subagents work in the project itself.
    #[must_use]
    pub fn with_worktrees(mut self, store: crate::worktree::WorktreeStore) -> Self {
        self.worktrees = Some(store);
        self
    }

    /// How long a subagent call waits for the round before moving it to the background.
    #[must_use]
    pub fn with_foreground_budget(mut self, budget: std::time::Duration) -> Self {
        self.background = self.background.with_budget(budget);
        self
    }

    /// Shares busy markers, message inboxes, and the round count with every tool built for the session.
    #[must_use]
    pub fn with_runtime(mut self, runtime: &SubagentRuntime) -> Self {
        self.busy = runtime.busy();
        self.background = self.background.sharing(runtime);
        self
    }

    /// Waits for a free round slot; workflow steps queue for one instead of failing at the cap.
    pub(crate) async fn wait_for_round_slot(&self) -> RoundSlot {
        self.background.wait_for_slot().await
    }

    #[cfg(test)]
    pub(crate) fn with_backgrounding(mut self, background: background::Backgrounding) -> Self {
        self.background = background;
        self
    }

    pub(crate) fn worktrees(&self) -> Option<&crate::worktree::WorktreeStore> {
        self.worktrees.as_ref()
    }

    /// Every subagent gets its own worktree: the store is set, the project is in git, and there is a
    /// session to save the round in (without one the round runs in the project itself).
    fn isolated(&self) -> bool {
        self.worktrees.is_some()
            && self.ctx.session.is_some()
            && self
                .ctx
                .project
                .as_ref()
                .is_some_and(|project| project.git_status.is_some())
    }

    async fn attach_worktree(&self, exchange: conversation::Exchange) -> Result<conversation::Exchange, String> {
        let label = format!("subagent {}", exchange.settings().name.as_deref().unwrap_or("task"));
        let owner = self.ctx.session.as_ref().map(|s| s.session_id.clone());
        match (&self.worktrees, &self.ctx.project) {
            (Some(store), Some(project)) => {
                exchange
                    .with_worktree(store, &project.current_dir, &label, owner.as_deref())
                    .await
            }
            _ => Ok(exchange),
        }
    }

    /// Configured model names for the `smol` and `slow` roles; `default` is always the session model.
    #[must_use]
    pub fn with_model_roles(mut self, smol: Option<String>, slow: Option<String>) -> Self {
        let configured = |m: Option<String>| m.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        for (role, model) in [(ModelRole::Smol, configured(smol)), (ModelRole::Slow, configured(slow))] {
            if let Some(model) = model {
                self.roles.insert(role, model);
            }
        }
        self
    }

    /// Model roles by lowercase built-in role or custom agent name, used when the caller passes none.
    #[must_use]
    pub fn with_pinned_models(mut self, pinned: BTreeMap<String, ModelRole>) -> Self {
        self.pinned_models = pinned;
        self
    }

    /// The pin in `[subagents.models]` for `name` (a role, agent, or persona), with the key it came from.
    fn pin_for(&self, name: &str) -> Option<(ModelRole, String)> {
        let key = name.to_ascii_lowercase();
        self.pinned_models.get(&key).map(|role| (*role, key))
    }

    fn pinned_model(&self, target: &Target<'_>) -> Option<(ModelRole, String)> {
        match target {
            Target::Agent(agent) => self.pin_for(&agent.name),
            Target::Role(role) => self.pin_for(role.as_str()),
        }
    }

    /// `Ok` if `name` is a defined persona, otherwise the error listing the valid ones.
    pub(crate) fn check_persona(&self, name: &str) -> Result<(), String> {
        self.resolve_persona(Some(name)).map(|_| ())
    }

    fn resolve_persona(&self, name: Option<&str>) -> Result<Option<&PersonaMetadata>, String> {
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        self.personas
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(Some)
            .ok_or_else(|| {
                let valid: Vec<&str> = self.personas.iter().map(|p| p.name.as_str()).collect();
                if valid.is_empty() {
                    format!("unknown persona '{name}'; none are defined (add .agents/personas/<name>.md)")
                } else {
                    format!("unknown persona '{name}'; valid: {}", valid.join(", "))
                }
            })
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
    async fn model_for(
        &self,
        role: Option<ModelRole>,
        default: &ModelHandle,
        resolver: Option<SubagentModelResolver>,
    ) -> Result<ModelHandle, String> {
        let name = match role {
            Some(ModelRole::Default) => return Ok(default.clone()),
            Some(role) => Some(self.roles.get(&role).ok_or_else(|| {
                format!(
                    "model role '{}' is not configured (set models.{} in config.toml)",
                    role.as_str(),
                    role.as_str()
                )
            })?),
            None => self.roles.get(&ModelRole::Smol),
        };
        let Some(name) = name else {
            return Ok(default.clone());
        };
        let resolver =
            resolver.ok_or_else(|| format!("model '{name}' requested but no model resolver is configured"))?;
        let name = name.clone();
        let label = name.clone();
        tokio::task::spawn_blocking(move || resolver(&name))
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
            .map_err(|e| format!("failed to resolve model '{label}' for subagent: {e}"))
    }

    pub(crate) async fn runner_for(&self, spec: RunnerSpec<'_>) -> Result<SubagentRunner, String> {
        let source = self.models.current();
        let default_model = source
            .default
            .as_ref()
            .ok_or_else(|| "No model handle configured for subagent execution.".to_string())?;
        let target = self.resolve_target(spec.agent, spec.local_agents)?;
        let persona = self.resolve_persona(spec.persona)?;
        let pinned = persona
            .and_then(|p| self.pin_for(&p.name))
            .or_else(|| self.pinned_model(&target));
        let requested = spec.model.or(pinned.as_ref().map(|(role, _)| *role));
        let model = self
            .model_for(requested, default_model, source.resolver.clone())
            .await
            .map_err(|e| match (&spec.model, &pinned) {
                (None, Some((_, key))) => format!("{e}; it is pinned for '{key}' in [subagents.models]"),
                _ => e,
            })?;
        let turns = resolve_turns(spec.turns);
        let runner = match &target {
            Target::Agent(agent) => {
                SubagentRunner::new_custom(model, agent.system_prompt.clone(), self.tools_for(&target), turns)
                    .with_label(&agent.name)
            }
            Target::Role(role) => SubagentRunner::new(model, *role, &self.tools, turns),
        };
        let runner = runner
            .with_context(self.ctx.clone())
            .with_keep_recent_tokens(self.keep_recent_tokens);
        let runner = match persona {
            Some(persona) => runner.with_persona(persona),
            None => runner,
        };
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
        role == ModelRole::Default || self.roles.contains_key(&role)
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
            .then(|| round::render_parent_conversation(&self.ctx.parent_history))
            .filter(|text| !text.is_empty());
        match (parent, args.context_slice.as_deref().filter(|s| !s.trim().is_empty())) {
            (Some(parent), Some(slice)) => Some(format!("{parent}\n\n{slice}")),
            (Some(text), None) => Some(text),
            (None, slice) => slice.map(str::to_string),
        }
    }

    fn unknown_target(&self, name: &str) -> String {
        let mut valid: Vec<&str> = SubagentRole::ALL.iter().map(SubagentRole::as_str).collect();
        valid.extend(self.agents.iter().map(|a| a.name.as_str()));
        format!("unknown subagent '{name}'; valid: {}", valid.join(", "))
    }

    /// `[subagents.models]` keys that name no built-in role, custom agent, or persona.
    fn unused_pins(&self) -> Vec<&str> {
        let known = |key: &str| {
            key.parse::<SubagentRole>().is_ok()
                || self.agents.iter().any(|a| a.name.eq_ignore_ascii_case(key))
                || self.personas.iter().any(|p| p.name.eq_ignore_ascii_case(key))
        };
        self.pinned_models
            .keys()
            .map(String::as_str)
            .filter(|key| !known(key))
            .collect()
    }
}

/// A custom agent gets the tools its `tools:` list names (never `subagent`, `lsp`, `memory`, or
/// `worktree`); without a list it
/// gets the read-only set a scout has, so an agent file has to ask for write or shell access.
fn filter_tools_for_agent(tools: &[Arc<dyn EngineTool>], agent: &AgentMetadata) -> Vec<Arc<dyn EngineTool>> {
    let listed = |names: &Option<Vec<String>>, name: &str| {
        names
            .as_ref()
            .map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(name)))
    };
    let read_only = SubagentRole::Scout.allowed_tools();
    runner::tools_named(tools, |name| {
        !matches!(name, "subagent" | "lsp" | "memory" | "worktree")
            && listed(&agent.tools, name).unwrap_or_else(|| read_only.contains(&name))
            && !listed(&agent.disallowed_tools, name).unwrap_or(false)
    })
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
        if parsed.inspect.unwrap_or(false) || self.isolated() {
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
