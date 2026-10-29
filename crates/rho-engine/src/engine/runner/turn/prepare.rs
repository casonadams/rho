use std::sync::Arc;

use crate::engine::AgentEngine;
use crate::engine::runner::history::continuation_history;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::SessionEventKind;

use super::runner::AgentRunner;
use super::types::{TurnOutput, TurnRequest};

pub(super) enum PreparedTurnOutcome {
    Compacted(Box<TurnOutput>),
    Ready(PreparedTurn),
}

pub(super) struct TurnRunnerParams<'a> {
    pub prompt: &'a str,
    pub preamble: &'a str,
    pub budget: usize,
    pub checkpoint: Option<&'a [ChatMessage]>,
    pub history: &'a [ChatMessage],
    pub stream_port: ToolStreamPort,
}

pub(super) struct PreparedTurn {
    pub preamble: String,
    pub sink: Arc<TerminalApprovalSink>,
    pub loop_state: TurnLoopState,
}

pub(super) struct TurnLoopState {
    pub visible_history: Vec<ChatMessage>,
    pub checkpoint: Option<Vec<ChatMessage>>,
    pub current_prompt: String,
    pub current_budget: usize,
    pub overflow_recovered: bool,
    pub rate_limit_retries: usize,
    pub network_retries: usize,
}

impl AgentEngine {
    fn start_turn_metrics(&self, additional_tokens: usize, history: &[ChatMessage]) {
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

    pub fn create_approval_sink(&self, presenter: &Arc<dyn Presenter>) -> Arc<TerminalApprovalSink> {
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

    pub(super) async fn load_initial_history(&self) -> Result<Vec<ChatMessage>> {
        let raw = self
            .session_manager
            .active_messages()
            .await
            .map_err(|e| AppError::Session(format!("Model-visible session history could not be loaded: {e}")))?;
        let window = self
            .context
            .limit_for(&self.config.model, &self.config.provider)
            .unwrap_or(128_000);
        let policy = super::prune::PrunePolicy::for_context_window(window).with_model(&self.config.model);
        Ok(super::prune::prune_historical_tool_outputs_with_policy(&raw, &policy))
    }

    pub(crate) async fn check_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        history: &mut Vec<ChatMessage>,
        additional_tokens: usize,
    ) -> Result<Option<crate::engine::CompactionStats>> {
        let history_tokens = self.usage.latest().map(|u| u.input_tokens as usize).unwrap_or_else(|| {
            history
                .iter()
                .map(|m| rho_harness_core::tokens::estimate_message_tokens(m, &self.config.model))
                .sum::<usize>()
        });
        let total_tokens = history_tokens.saturating_add(additional_tokens);
        let limit = crate::engine::model::resolve_context_limit(&self.config).unwrap_or(128_000);
        let threshold = limit.saturating_sub(self.config.reserve_tokens);
        if total_tokens > threshold {
            let _spinner = presenter.start_spinner("Compacting...");
            let stats = self.compact_session(None).await?;
            *history = self.session_manager.active_messages().await?;
            presenter.print_notice(&format!(
                "[Auto-compacted context: {} -> {} tokens (saved {})]",
                stats.tokens_before, stats.tokens_after, stats.saved_tokens
            ));
            Ok(Some(stats))
        } else {
            Ok(None)
        }
    }

    async fn check_turn_proactive_compaction(
        &self,
        preamble: &str,
        prompt: &str,
        presenter: &dyn Presenter,
        history: &mut Vec<ChatMessage>,
    ) -> Result<Option<TurnOutput>> {
        let add_tokens = rho_harness_core::tokens::estimate_text_tokens(preamble, &self.config.model).saturating_add(
            rho_harness_core::tokens::estimate_text_tokens(prompt, &self.config.model),
        );
        if self
            .check_proactive_compaction(presenter, history, add_tokens)
            .await?
            .is_some()
        {
            return self.compacted_turn_output().await.map(Some);
        }
        Ok(None)
    }

    async fn build_ready_turn(
        &self,
        user_prompt: &str,
        effective_prompt: &str,
        preamble: String,
        history: Vec<ChatMessage>,
        presenter: &Arc<dyn Presenter>,
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
            network_retries: 0,
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
            .check_turn_proactive_compaction(&preamble, &effective_prompt, presenter.as_ref(), &mut history)
            .await?
        {
            return Ok(PreparedTurnOutcome::Compacted(Box::new(out)));
        }
        self.build_ready_turn(prompt, &effective_prompt, preamble, history, presenter)
            .await
            .map(PreparedTurnOutcome::Ready)
    }

    async fn build_turn_runner<'a>(
        &self,
        params: TurnRunnerParams<'a>,
        sink: Arc<TerminalApprovalSink>,
        request: &TurnRequest<'_>,
    ) -> AgentRunner {
        let tools = self.tools.read().await.clone();
        let model = self.model.clone().expect("Model must be initialized");
        let mut runner = AgentRunner::new(model, tools, self.session_manager.clone(), sink)
            .preamble(params.preamble)
            .prompt(params.prompt)
            .budget(params.budget)
            .stream_port(params.stream_port)
            .steering(request.steering.clone())
            .working_dir(self.base_dir.clone())
            .quality_gate(self.config.quality_gate);

        if let Some(pending) = params.checkpoint {
            let combined = continuation_history(params.history, pending);
            runner = runner.history(combined);
        } else {
            runner = runner.history(params.history.to_vec());
        }
        runner
    }

    pub(super) async fn prepare_step_runner<'a>(
        &self,
        sink: &Arc<TerminalApprovalSink>,
        preamble: &'a str,
        request: &TurnRequest<'_>,
        presenter: &Arc<dyn Presenter>,
        loop_state: &'a TurnLoopState,
    ) -> Result<(AgentRunner, String)> {
        let active_model = request
            .model_switch
            .as_ref()
            .and_then(|s| s.current_model())
            .unwrap_or_else(|| self.config.model.clone());

        let cwd = self.base_dir.clone();
        let mut hook =
            crate::hook::LifecycleHook::new(cwd.clone(), self.session_manager.session_id.clone(), presenter.clone());
        if !self.config.hook_dirs.is_empty() {
            hook = hook.with_hook_dirs(self.config.hook_dirs.clone());
        }
        let lifecycle_hook = Arc::new(hook);
        lifecycle_hook.notify_turn_start(&loop_state.current_prompt).await;

        let permission_hook = if self.config.permission.enabled {
            let mut perm = crate::permission::PermissionHook::new(Some(cwd), presenter.clone());
            let auth_store = self.auth_store.lock().await;
            if let Ok(Some(guard_model)) =
                crate::provider::ProviderFactory::create_guard_model(&self.config, &auth_store)
            {
                perm = perm.with_guard(crate::permission::guard::GuardEvaluator::new(guard_model));
            }
            drop(auth_store);
            Some(Arc::new(perm))
        } else {
            None
        };

        let runner = self
            .build_turn_runner(
                TurnRunnerParams {
                    prompt: &loop_state.current_prompt,
                    preamble,
                    budget: loop_state.current_budget,
                    checkpoint: loop_state.checkpoint.as_deref(),
                    history: &loop_state.visible_history,
                    stream_port: presenter.stream_port(),
                },
                sink.clone(),
                request,
            )
            .await
            .lifecycle(Some(lifecycle_hook))
            .permission(permission_hook)
            .cancellation(request.cancellation.cloned());

        Ok((runner, active_model))
    }

    pub(crate) async fn apply_model_routing(&self, request: &mut TurnRequest<'_>, presenter: &dyn Presenter) {
        if !self.config.routing {
            return;
        }
        let Some(router) = crate::routing::ModelRouter::from_config(&self.config) else {
            return;
        };

        let verdict = router.route_prompt(request.prompt).await;
        if verdict.fallback {
            return;
        }

        let confidence_str = verdict
            .confidence
            .map(|c| format!(" ({:.0}%)", c * 100.0))
            .unwrap_or_default();
        presenter.print_notice(&format!(
            "Routed to {} tier: {}{}",
            verdict.tier, verdict.model, confidence_str
        ));

        request.routed_model = Some(verdict.model);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_resolve_active_provider() {
        let (spec_p, _) = rho_harness_core::provider::parse_model_spec("antigravity/gemini-3.8-flash");
        assert_eq!(spec_p, "antigravity");
        let (spec_p2, _) = rho_harness_core::provider::parse_model_spec("gemini-3.8-flash");
        assert!(spec_p2.is_empty());
    }
}
