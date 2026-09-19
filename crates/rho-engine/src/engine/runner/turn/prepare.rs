use std::sync::Arc;

use crate::engine::AgentEngine;
use crate::engine::runner::history::continuation_history;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::runtime::build_runner;
use crate::repeat::RepeatedCallHook;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::SessionEventKind;
use rig::agent::AgentRunner;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::tool::ToolContext;

use super::tool_hook::TurnToolExecutionHook;
use super::types::{TurnOutput, TurnRequest};

pub(super) enum PreparedTurnOutcome {
    Compacted(Box<TurnOutput>),
    Ready(PreparedTurn),
}

pub(super) struct PreparedTurn {
    pub preamble: String,
    pub sink: Arc<TerminalApprovalSink>,
    pub loop_state: TurnLoopState,
}

pub(super) struct TurnLoopState {
    pub visible_history: Vec<Message>,
    pub checkpoint: Option<Vec<Message>>,
    pub current_prompt: String,
    pub current_budget: usize,
    pub overflow_recovered: bool,
    pub rate_limit_retries: usize,
}

impl AgentEngine {
    fn start_turn_metrics(&self, additional_tokens: usize, history: &[Message]) {
        self.run_tracker.start();
        let anchor = if !history.is_empty() {
            self.usage
                .latest()
                .filter(|u| u.has_values())
                .map(|u| (history.len() - 1, self.consumed_context(&u) as usize))
                .filter(|&(_, tokens)| tokens > 0)
        } else {
            None
        };
        let hist_tokens = self
            .context
            .calculate_context_tokens(history, anchor, &self.config.model)
            .total_tokens;
        let est = additional_tokens.saturating_add(hist_tokens) as u64;
        self.usage.start_turn(Some(est));
    }

    fn create_approval_sink(&self, presenter: &Arc<dyn Presenter>) -> Arc<TerminalApprovalSink> {
        let model_label = format!("{}:{}", self.config.model, self.context_usage_display());
        TerminalApprovalSink::new(
            presenter,
            TerminalSinkConfig {
                model_label,
                run_tracker: self.run_tracker.clone(),
            },
            self.session_manager.clone(),
        )
    }

    async fn load_initial_history(&self) -> Result<Vec<Message>> {
        let raw = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        Ok(super::prune::prune_historical_tool_outputs(
            &raw,
            1,
            super::prune::DEFAULT_PRUNE_LINE_THRESHOLD,
        ))
    }

    async fn check_turn_proactive_compaction(
        &self,
        (preamble, prompt): (&str, &str),
        (presenter, history): (&dyn Presenter, &mut Vec<Message>),
    ) -> Result<Option<TurnOutput>> {
        let add_tokens = rho_harness_core::tokens::estimate_text_tokens(preamble, &self.config.model).saturating_add(
            rho_harness_core::tokens::estimate_text_tokens(prompt, &self.config.model),
        );
        if self
            .check_proactive_compaction(presenter, (history, add_tokens))
            .await?
            .is_some()
        {
            return self.compacted_turn_output().await.map(Some);
        }
        Ok(None)
    }

    async fn build_ready_turn(
        &self,
        ((user_prompt, effective_prompt), preamble): ((&str, &str), String),
        (history, presenter): (Vec<Message>, &Arc<dyn Presenter>),
    ) -> Result<PreparedTurn> {
        self.session_manager
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({ "prompt": user_prompt }),
            )
            .await?;
        let checkpoint = self.session_manager.load_checkpoint().await?;
        let add_tokens = rho_harness_core::tokens::estimate_text_tokens(&preamble, &self.config.model).saturating_add(
            rho_harness_core::tokens::estimate_text_tokens(effective_prompt, &self.config.model),
        );
        self.start_turn_metrics(add_tokens, &history);
        let sink = self.create_approval_sink(presenter);
        let loop_state = TurnLoopState {
            visible_history: history,
            checkpoint,
            current_prompt: effective_prompt.to_string(),
            current_budget: self.config.max_turns,
            overflow_recovered: false,
            rate_limit_retries: 0,
        };
        Ok(PreparedTurn {
            preamble,
            sink,
            loop_state,
        })
    }

    pub(super) async fn prepare_turn(
        &self,
        prompt: &str,
        presenter: &Arc<dyn Presenter>,
    ) -> Result<PreparedTurnOutcome> {
        let context = self.project_context().await?;
        let preamble = context.build_system_prompt();
        let effective_prompt = crate::engine::context::format_turn_prompt(prompt, context.git_status.as_deref());
        let mut history = self.load_initial_history().await?;
        if let Some(out) = self
            .check_turn_proactive_compaction((&preamble, &effective_prompt), (presenter.as_ref(), &mut history))
            .await?
        {
            return Ok(PreparedTurnOutcome::Compacted(Box::new(out)));
        }
        self.build_ready_turn(((prompt, &effective_prompt), preamble), (history, presenter))
            .await
            .map(PreparedTurnOutcome::Ready)
    }

    async fn build_turn_hooks(
        &self,
        (sink, request): (&Arc<TerminalApprovalSink>, &TurnRequest<'_>),
        (presenter, prompt): (&Arc<dyn Presenter>, &str),
    ) -> Result<rig::agent::hook::HookStack> {
        let cwd = self.base_dir.clone();
        let lifecycle_hook =
            crate::hook::LifecycleHook::new(cwd.clone(), self.session_manager.session_id.clone(), presenter.clone());
        lifecycle_hook.notify_turn_start(prompt).await;

        let mut hook_stack = rig::agent::hook::HookStack::new();
        hook_stack.push(RepeatedCallHook::new(cwd.clone()));
        hook_stack.push(lifecycle_hook);
        if self.config.permission.enabled {
            hook_stack.push(crate::permission::PermissionHook::new(Some(cwd), presenter.clone()));
        }
        let mut auto_compact = super::auto_compact::AutoCompactHook::new(
            self.session_compactor(),
            presenter.clone(),
            self.usage.clone(),
            self.context.clone(),
            &self.config.provider,
            self.config.reserve_tokens,
        );
        if let Some(hook) = &self.demotion_hook {
            auto_compact = auto_compact.with_demotion_hook(Arc::clone(hook));
        }
        hook_stack.push(auto_compact);
        let ceiling = crate::engine::model::resolve_context_limit(&self.config)
            .map(|l| l as u64)
            .unwrap_or(65536);
        hook_stack.push(super::truncation_hook::TruncationRecoveryHook::new(
            self.config.max_output_tokens,
            ceiling,
            2,
        ));
        hook_stack.push(
            TurnToolExecutionHook::new(sink.clone(), &self.config.provider, request.steering.clone())
                .with_model_switch(request.model_switch.clone())
                .with_project_context(self.project_context.clone()),
        );
        Ok(hook_stack)
    }

    async fn build_turn_runner<'a>(
        &self,
        (prompt, preamble, budget): (&'a str, &'a str, usize),
        (checkpoint, history, hooks, stream_port): (
            Option<&[Message]>,
            &[Message],
            rig::agent::hook::HookStack,
            ToolStreamPort,
        ),
    ) -> AgentRunner {
        let mut tool_context = ToolContext::new();
        tool_context.insert(stream_port);
        let agent_guard = self.agent.read().await;
        let runner = build_runner(&agent_guard, prompt)
            .conversation(self.session_manager.session_id.clone())
            .preamble(preamble)
            .max_turns(budget)
            .tool_context(tool_context)
            .add_hook(hooks);
        drop(agent_guard);
        match checkpoint {
            Some(pending) => runner.history(continuation_history(history, pending)),
            None => runner,
        }
    }

    pub(super) async fn prepare_step_runner<'a>(
        &self,
        (sink, preamble, request, presenter): (
            &Arc<TerminalApprovalSink>,
            &'a str,
            &TurnRequest<'_>,
            &Arc<dyn Presenter>,
        ),
        loop_state: &'a TurnLoopState,
    ) -> Result<(AgentRunner, String)> {
        let active_model = request
            .model_switch
            .as_ref()
            .and_then(|s| s.current_model())
            .unwrap_or_else(|| self.config.model.clone());
        let hooks = self
            .build_turn_hooks((sink, request), (presenter, &loop_state.current_prompt))
            .await?;
        let runner = self
            .build_turn_runner(
                (&loop_state.current_prompt, preamble, loop_state.current_budget),
                (
                    loop_state.checkpoint.as_deref(),
                    &loop_state.visible_history,
                    hooks,
                    presenter.stream_port(),
                ),
            )
            .await;
        Ok((runner, active_model))
    }
}
