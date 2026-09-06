mod completion;
mod stream;
mod streaming_tool;
mod tool_hook;
pub mod types;

pub use types::{
    ActiveModelSwitch, CancellationSignal, QUEUED_MESSAGE_BOUNDARY, QueuedMessageBoundary, RunStatus,
    SharedModelSwitch, SteeringQueueProvider, TurnOutput, TurnRequest, UsageDetails,
};

use crate::engine::AgentEngine;
use crate::engine::runtime::build_runner;
use crate::plugin::daemon::DaemonHook;
use crate::repeat::RepeatedCallHook;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::SessionEventKind;
use rig::agent::AgentRunner;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::tool::ToolContext;
use std::sync::Arc;
use stream::StreamRunResult;
use tool_hook::TurnToolExecutionHook;

use super::history::continuation_history;
use super::sink::{TerminalApprovalSink, TerminalSinkConfig};

struct PreparedTurn {
    preamble: String,
    sink: Arc<TerminalApprovalSink>,
    loop_state: TurnLoopState,
}

struct TurnLoopState {
    visible_history: Vec<Message>,
    checkpoint: Option<Vec<Message>>,
    current_prompt: String,
    current_budget: usize,
    overflow_recovered: bool,
}

impl AgentEngine {
    async fn perform_proactive_compaction(&self, presenter: &dyn Presenter, history: &mut Vec<Message>) -> Result<()> {
        let spinner = presenter.start_spinner("Compacting...");
        match self.compact_session(None).await {
            Ok(stats) => {
                spinner.finish_and_clear();
                presenter.print_notice(&format!(
                    "[Auto-compacted context: {} -> {} tokens (saved {})]",
                    stats.tokens_before, stats.tokens_after, stats.saved_tokens
                ));
                *history = ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
                    .await
                    .map_err(|e| {
                        AppError::Session(format!("Model-visible session history could not be loaded: {e}"))
                    })?;
            }
            Err(err) => {
                spinner.finish_and_clear();
                eprintln!("Warning: Proactive auto-compaction failed: {err}");
            }
        }
        Ok(())
    }

    async fn check_proactive_compaction(&self, presenter: &dyn Presenter, history: &mut Vec<Message>) -> Result<()> {
        let window = self
            .context_limit()
            .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&self.config.model));
        let tokens = rho_harness_core::tokens::calculate_context_tokens(history, None, &self.config.model).total_tokens;
        if rho_harness_core::tokens::should_compact(tokens, window, self.config.reserve_tokens) {
            self.perform_proactive_compaction(presenter, history).await?;
        }
        Ok(())
    }

    fn start_turn_metrics(&self, (preamble, prompt): (&str, &str), history: &[Message]) {
        self.run_tracker.start();
        let preamble_tokens = rho_harness_core::tokens::estimate_text_tokens(preamble, &self.config.model);
        let prompt_tokens = rho_harness_core::tokens::estimate_text_tokens(prompt, &self.config.model);
        let hist_tokens =
            rho_harness_core::tokens::calculate_context_tokens(history, None, &self.config.model).total_tokens;
        let est = preamble_tokens
            .saturating_add(hist_tokens)
            .saturating_add(prompt_tokens) as u64;
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
        ConversationMemory::load(&self.session_manager, &self.session_manager.session_id)
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))
    }

    async fn record_user_prompt(&self, prompt: &str) -> Result<String> {
        let context = self.project_context().await?;
        self.session_manager
            .append_event(SessionEventKind::UserMessage, serde_json::json!({ "prompt": prompt }))
            .await?;
        Ok(context.build_system_prompt())
    }

    async fn load_turn_history(&self, presenter: &dyn Presenter) -> Result<(Vec<Message>, Option<Vec<Message>>)> {
        let mut history = self.load_initial_history().await?;
        let checkpoint = self.session_manager.load_checkpoint().await?;
        self.check_proactive_compaction(presenter, &mut history).await?;
        Ok((history, checkpoint))
    }

    async fn prepare_turn(&self, prompt: &str, presenter: &Arc<dyn Presenter>) -> Result<PreparedTurn> {
        let preamble = self.record_user_prompt(prompt).await?;
        let (history, checkpoint) = self.load_turn_history(presenter.as_ref()).await?;
        self.start_turn_metrics((&preamble, prompt), &history);
        let sink = self.create_approval_sink(presenter);
        let loop_state = TurnLoopState {
            visible_history: history,
            checkpoint,
            current_prompt: prompt.to_string(),
            current_budget: self.config.max_turns,
            overflow_recovered: false,
        };
        Ok(PreparedTurn {
            preamble,
            sink,
            loop_state,
        })
    }

    async fn build_turn_hooks(
        &self,
        (sink, request): (&Arc<TerminalApprovalSink>, &TurnRequest<'_>),
        (presenter, prompt): (&Arc<dyn Presenter>, &str),
    ) -> Result<rig::agent::hook::HookStack> {
        let cwd = std::env::current_dir()?;
        let plugin_hook = DaemonHook::new(&self.config.plugins, &cwd, presenter.clone()).await;
        plugin_hook.notify_turn_start(prompt).await;

        let mut hook_stack = rig::agent::hook::HookStack::new();
        hook_stack.push(RepeatedCallHook::new(cwd.clone()));
        hook_stack.push(plugin_hook);
        for p in &self.plugins {
            p.register_hooks(&mut hook_stack);
        }
        if self.config.permission.enabled && !crate::permission::has_external_permission_plugin(&self.config.plugins) {
            hook_stack.push(crate::permission::PermissionHook::new(Some(cwd), presenter.clone()));
        }
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

    async fn prepare_step_runner<'a>(
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

    async fn handle_stream_run_result(
        &self,
        (res, sink): (StreamRunResult, &Arc<TerminalApprovalSink>),
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        match res {
            StreamRunResult::RetryOverflow => Ok(None),
            StreamRunResult::BudgetContinue => {
                sink.resume_model_spinner();
                loop_state.current_prompt = "Please continue where you left off and finish the task.".to_string();
                loop_state.current_budget = 50;
                Ok(None)
            }
            StreamRunResult::Complete(state) => {
                let out = self
                    .finalize_turn_execution(*state, (sink, loop_state.checkpoint.as_deref()))
                    .await?;
                Ok(Some(out))
            }
        }
    }

    async fn execute_turn_step(
        &self,
        (sink, preamble, request, presenter): (&Arc<TerminalApprovalSink>, &str, &TurnRequest<'_>, &Arc<dyn Presenter>),
        loop_state: &mut TurnLoopState,
    ) -> Result<Option<TurnOutput>> {
        let (runner, active_model) = self
            .prepare_step_runner((sink, preamble, request, presenter), loop_state)
            .await?;
        let stream_res = self
            .run_turn_stream(
                (runner, sink, presenter.as_ref()),
                (
                    &active_model,
                    &mut loop_state.visible_history,
                    &mut loop_state.checkpoint,
                    &mut loop_state.overflow_recovered,
                ),
            )
            .await?;
        self.handle_stream_run_result((stream_res, sink), loop_state).await
    }

    pub async fn run_turn(
        &self,
        request: TurnRequest<'_>,
        presenter: std::sync::Arc<dyn Presenter>,
    ) -> Result<TurnOutput> {
        let mut prep = self.prepare_turn(request.prompt, &presenter).await?;
        let _in_flight_guard = self.usage.in_flight_guard();
        loop {
            if let Some(out) = self
                .execute_turn_step((&prep.sink, &prep.preamble, &request, &presenter), &mut prep.loop_state)
                .await?
            {
                return Ok(out);
            }
        }
    }
}
