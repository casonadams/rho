//! A subagent task's lifecycle: starting one, continuing or steering it, inspecting saved tasks,
//! and running each round in the foreground or background.

use super::{RawSubagentArgs, RunnerSpec, SubagentRole, SubagentRunner, SubagentTool, round};
use super::{background, conversation, inspect, types, workspace};
use crate::tools::types::ToolResult;
use rho_harness_core::agents::AgentMetadata;
use rho_harness_core::workflow::ModelRole;

impl SubagentTool {
    async fn runner_for_settings(
        &self,
        settings: &conversation::ConversationSettings,
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
            turns: settings.turns,
            model,
            position: None,
            task_name: settings.name.clone(),
            persona: settings.persona.as_deref(),
        })
        .await
    }

    pub(super) async fn inspect(&self, task_id: Option<&str>) -> ToolResult {
        let Some(session) = &self.ctx.session else {
            return ToolResult::error("inspecting subagent tasks needs a saved session");
        };
        let Some(task_id) = task_id.filter(|id| !id.is_empty()) else {
            let mut listing = inspect::render_list(&session.subagent_contexts().await);
            let unused = self.unused_pins();
            if !unused.is_empty() {
                listing.push_str(&format!(
                    "\n\n[subagents.models] keys that match no role, agent, or persona (ignored): {}",
                    unused.join(", ")
                ));
            }
            return ToolResult::success(listing);
        };
        if let Some(running) = self.background.still_running(task_id) {
            return ToolResult::error(running);
        }
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

    pub(super) async fn start_task(&self, args: &RawSubagentArgs) -> Result<ToolResult, String> {
        let settings = conversation::ConversationSettings {
            name: args.name.as_deref().and_then(types::task_name),
            agent: args.agent.clone().or_else(|| args.role.clone()),
            model: args.model.clone(),
            agent_file: None,
            persona: args.persona.clone(),
            turns: args.turns,
        };
        let runner = self.runner_for_settings(&settings).await?;
        let context = self.compose_context(args);
        let slot = self.background.reserve()?;
        if self.ctx.session.is_none() {
            let result = runner.run(&args.prompt, context.as_deref()).await;
            drop(slot);
            return Ok(result);
        }
        let exchange = conversation::Exchange::start(&settings).claimed(&self.busy)?;
        let exchange = self.attach_worktree(exchange).await?;
        let input = round::build_subagent_prompt(&args.prompt, context.as_deref());
        Ok(self.converse(slot, exchange, runner, input, None).await)
    }

    pub(super) async fn continue_task(&self, args: &RawSubagentArgs, task_id: &str) -> Result<ToolResult, String> {
        let session = self
            .ctx
            .session
            .as_ref()
            .ok_or("continuing a subagent task needs a saved session")?;
        if let Some(settings) = self.background.running_settings(task_id) {
            check_continuation(&settings, args, task_id)?;
            if args.model.is_some() || args.turns.is_some() {
                return Err(format!(
                    "task '{task_id}' is still running, so `model` and `turns` cannot change; omit them to send it a message, or pass them after its result arrives"
                ));
            }
            let message = round::build_subagent_prompt(&args.prompt, self.compose_context(args).as_deref());
            if let Some(queued) = self.background.steer(task_id, &message) {
                return Ok(ToolResult::success(queued));
            }
        }
        if let Some(running) = self.background.still_running(task_id) {
            return Err(running);
        }
        let context = session
            .subagent_context(task_id)
            .await
            .ok_or_else(|| format!("unknown subagent task '{task_id}'"))?;
        let exchange = conversation::Exchange::resume(context, task_id)?;
        let mut settings = exchange.settings();
        check_continuation(&settings, args, task_id)?;
        settings.model = args.model.clone().or(settings.model);
        settings.turns = args.turns.or(settings.turns);
        let runner = self.runner_for_settings(&settings).await?;
        let slot = self.background.reserve()?;
        let exchange = self
            .attach_worktree(exchange.with_settings(&settings).claimed(&self.busy)?)
            .await?;
        let input = round::build_subagent_prompt(&args.prompt, self.compose_context(args).as_deref());
        Ok(self
            .converse(slot, exchange, runner, input, Some(task_id.to_string()))
            .await)
    }

    /// Runs one round on its own task, which moves to the background if it outlasts the foreground budget.
    async fn converse(
        &self,
        slot: background::RoundSlot,
        exchange: conversation::Exchange,
        runner: SubagentRunner,
        input: String,
        reference: Option<String>,
    ) -> ToolResult {
        let Some(session) = self.ctx.session.clone() else {
            return ToolResult::error("a subagent task needs a saved session");
        };
        let task_id = exchange.task_id().to_string();
        let settings = exchange.settings();
        let label = format!(
            "subagent {}",
            settings
                .name
                .or(settings.agent)
                .as_deref()
                .unwrap_or(SubagentRole::default().as_str())
        );
        let runner = runner.with_parent_channel();
        let owner = session.session_id.clone();
        let task = background::BackgroundTask {
            id: &task_id,
            owner: &owner,
            label,
            cancellation: self.ctx.cancellation.clone(),
            settings: exchange.settings(),
            presenter: (self.ctx.a2ui_widgets && self.ctx.a2ui_config.subagent_widget)
                .then(|| self.ctx.presenter.clone())
                .flatten(),
        };
        self.background
            .run(slot, task, &self.ctx.progress, move |io| async move {
                let runner = runner
                    .with_progress(io.progress)
                    .with_inbox(io.inbox)
                    .with_cancellation(io.stop);
                exchange.converse(&runner, &session, &input, reference.as_deref()).await
            })
            .await
    }
}

/// Refuses a role, agent, or persona that differs from the one the task already runs as.
fn check_continuation(
    settings: &conversation::ConversationSettings,
    args: &RawSubagentArgs,
    task_id: &str,
) -> Result<(), String> {
    let differs = |requested: &str, current: Option<&str>| !current.is_some_and(|c| c.eq_ignore_ascii_case(requested));
    let agent = settings.agent.as_deref().unwrap_or(SubagentRole::default().as_str());
    if let Some(requested) = args
        .agent
        .as_deref()
        .or(args.role.as_deref())
        .filter(|r| differs(r, Some(agent)))
    {
        return Err(format!(
            "task '{task_id}' runs as '{agent}', not '{requested}'; omit role and agent when continuing a task"
        ));
    }
    if let Some(requested) = args
        .persona
        .as_deref()
        .filter(|r| differs(r, settings.persona.as_deref()))
    {
        return Err(format!(
            "task '{task_id}' does not use persona '{requested}'; omit persona when continuing a task"
        ));
    }
    Ok(())
}
