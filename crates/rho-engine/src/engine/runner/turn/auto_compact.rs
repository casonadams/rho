use std::sync::Arc;

use crate::engine::AgentEngine;
use crate::engine::compactor::SessionCompactor;
use crate::engine::compactor::llm::{LlmCompactor, SummarizeOptions};
use crate::engine::metrics::StructuralUsage;
use crate::engine::tracking::{ContextTracker, UsageTracker};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::compaction::compaction_summary_message;
use rho_harness_core::tokens::{context_window_size_for_provider, find_token_cut_point, should_compact};
use rig::agent::hook::{AgentHook, CompletionCall, CompletionCallAction, HookContext, RequestPatch};
use rig::memory::ConversationMemory;
use rig::message::Message;

/// Provider-anchored context size: input, output, and cache reads/writes of the
/// latest model call.
fn usage_anchor_tokens(usage: &StructuralUsage, provider: &str) -> usize {
    let consumed = crate::engine::display::consumed_context_tokens(usage, provider);
    consumed.saturating_add(usage.output_tokens) as usize
}

fn estimated_tokens(messages: &[Message], model: &str, context: &ContextTracker) -> usize {
    context.calculate_context_tokens(messages, None, model).total_tokens
}

fn trigger_tokens(
    messages: &[Message],
    usage: Option<&StructuralUsage>,
    model: &str,
    provider: &str,
    context: &ContextTracker,
) -> usize {
    let anchor_tokens = usage.map(|u| usage_anchor_tokens(u, provider)).unwrap_or(0);
    if anchor_tokens > 0 {
        let anchor_idx = messages.iter().rposition(|m| matches!(m, Message::Assistant { .. }));
        if let Some(idx) = anchor_idx {
            context
                .calculate_context_tokens(messages, Some((idx, anchor_tokens)), model)
                .total_tokens
        } else {
            let full_estimate = estimated_tokens(messages, model, context);
            anchor_tokens.max(full_estimate)
        }
    } else {
        estimated_tokens(messages, model, context)
    }
}

fn context_window(model: &str, provider: &str, context: &ContextTracker) -> usize {
    context
        .limit_for(model, provider)
        .unwrap_or_else(|| context_window_size_for_provider(model, provider))
}

/// Replacement history for a compacted model call: `prefix` stands in for
/// `history[..cut]`, and everything from `cut` on is kept verbatim.
#[derive(Default, Clone)]
pub(crate) struct CompactState {
    pub(crate) base_len: Option<usize>,
    pub(crate) tripped: bool,
    pub(crate) patch: Option<PatchPlan>,
}

#[derive(Debug, Clone)]
pub(crate) struct PatchPlan {
    cut: usize,
    prefix: Vec<Message>,
}

impl PatchPlan {
    fn apply(&self, history: &[Message]) -> Vec<Message> {
        let mut messages = self.prefix.clone();
        messages.extend_from_slice(&history[self.cut.min(history.len())..]);
        messages
    }
}

/// Auto-compaction boundary: after tools finish and before the next assistant
/// response, compact when the context exceeds the window minus the response
/// reserve, and continue the same run on the compacted context via a per-call
/// history patch.
pub(crate) struct AutoCompactHook {
    compactor: SessionCompactor,
    presenter: Arc<dyn Presenter>,
    usage: UsageTracker,
    context: ContextTracker,
    provider: String,
    reserve_tokens: usize,
    demotion_hook: Option<Arc<dyn rig::memory::DemotionHook>>,
}

impl AutoCompactHook {
    pub(crate) fn new(
        compactor: SessionCompactor,
        presenter: Arc<dyn Presenter>,
        usage: UsageTracker,
        context: ContextTracker,
        provider: &str,
        reserve_tokens: usize,
    ) -> Self {
        let demotion_hook = compactor.demotion_hook().cloned();
        Self {
            compactor,
            presenter,
            usage,
            context,
            provider: provider.to_string(),
            reserve_tokens,
            demotion_hook,
        }
    }

    pub(crate) fn with_demotion_hook(mut self, hook: Arc<dyn rig::memory::DemotionHook>) -> Self {
        self.demotion_hook = Some(hook);
        self
    }

    fn model_name(&self) -> &str {
        self.compactor.model_name()
    }

    async fn compact_and_plan(&self, history: &[Message], base_len: usize) -> Option<PatchPlan> {
        let spinner = self.presenter.start_spinner("Compacting...");
        match self.compactor.compact(None).await {
            Ok(stats) if !stats.summary.is_empty() => {
                spinner.finish_and_clear();
                self.presenter.print_notice(&format!(
                    "[Auto-compacted context: {} -> {} tokens (saved {})]",
                    stats.tokens_before, stats.tokens_after, stats.saved_tokens
                ));
                let session_manager = self.compactor.session_manager();
                match ConversationMemory::load(session_manager, &session_manager.session_id).await {
                    Ok(reloaded) => Some(PatchPlan {
                        cut: base_len,
                        prefix: reloaded,
                    }),
                    Err(err) => {
                        eprintln!("Warning: Mid-run compaction history reload failed: {err}");
                        None
                    }
                }
            }
            // Nothing durable to summarize (tree already compact or under the
            // keep-recent budget): summarize the flat prefix of this run.
            Ok(_) => {
                spinner.finish_and_clear();
                self.ephemeral_plan(history).await
            }
            Err(err) => {
                spinner.finish_and_clear();
                eprintln!("Warning: Mid-run auto-compaction failed: {err}");
                self.ephemeral_plan(history).await
            }
        }
    }

    async fn ephemeral_plan(&self, history: &[Message]) -> Option<PatchPlan> {
        let model = self.model_name();
        let cut = find_token_cut_point(history, self.compactor.keep_recent_tokens(), model);
        if cut.cut_index == 0 {
            return None;
        }
        let evicted = &history[..cut.cut_index];
        crate::engine::compactor::orchestrator::dispatch_demote(
            self.demotion_hook.as_ref(),
            &self.compactor.session_manager().session_id,
            evicted,
        )
        .await;
        let compactor = LlmCompactor::new(self.compactor.model().cloned());
        let summary = compactor
            .summarize(
                &history[..cut.cut_index],
                SummarizeOptions {
                    prior_summary: None,
                    custom_instructions: None,
                    is_split_turn: cut.is_split_turn,
                    structured: false,
                },
            )
            .await;
        let summary = self.compactor.session_manager().redact_credentials(&summary);
        let summary = if self.compactor.max_bytes() > 0 && summary.len() > self.compactor.max_bytes() {
            let mut end = self.compactor.max_bytes();
            while end > 0 && !summary.is_char_boundary(end) {
                end -= 1;
            }
            summary[..end].to_string()
        } else {
            summary
        };
        let summary_message = compaction_summary_message(&summary);
        let mut kept_with_summary = vec![summary_message.clone()];
        kept_with_summary.extend_from_slice(&history[cut.cut_index..]);
        let tokens_after = self
            .context
            .calculate_context_tokens(&kept_with_summary, None, model)
            .total_tokens;
        self.usage.record(StructuralUsage {
            input_tokens: tokens_after as u64,
            ..Default::default()
        });
        Some(PatchPlan {
            cut: cut.cut_index,
            prefix: vec![summary_message],
        })
    }
}

impl AgentHook for AutoCompactHook {
    async fn on_completion_call(&self, ctx: &HookContext, event: CompletionCall<'_>) -> CompletionCallAction {
        self.handle(Some(ctx), event.history, event.prompt).await
    }
}

impl AutoCompactHook {
    pub(crate) async fn handle(
        &self,
        ctx: Option<&HookContext>,
        history: &[Message],
        prompt: &Message,
    ) -> CompletionCallAction {
        let pruned =
            super::prune::prune_historical_tool_outputs(history, 1, super::prune::DEFAULT_PRUNE_LINE_THRESHOLD);
        let was_pruned = pruned != history;
        let effective_history = if was_pruned { &pruned } else { history };

        let mut local_state = CompactState::default();
        let (tripped, base_len) = if let Some(c) = ctx {
            c.scratchpad().update::<CompactState, _>(|state| {
                state.base_len.get_or_insert(effective_history.len());
                (state.tripped, state.base_len.unwrap_or(effective_history.len()))
            })
        } else {
            local_state.base_len = Some(effective_history.len());
            (local_state.tripped, effective_history.len())
        };

        if !tripped {
            let window = context_window(self.model_name(), &self.provider, &self.context);
            let prompt_tokens = self.context.estimate_message_tokens(prompt, self.model_name());
            let messages = trigger_tokens(
                effective_history,
                self.usage.latest().as_ref(),
                self.model_name(),
                &self.provider,
                &self.context,
            )
            .saturating_add(prompt_tokens);
            if should_compact(messages, window, self.reserve_tokens) {
                let plan = self.compact_and_plan(effective_history, base_len).await;
                if let Some(c) = ctx {
                    c.scratchpad().update::<CompactState, _>(|state| {
                        state.tripped = true;
                        state.patch = plan;
                    });
                } else {
                    local_state.tripped = true;
                    local_state.patch = plan;
                }
            }
        }
        let replacement = if let Some(c) = ctx {
            c.scratchpad()
                .get::<CompactState>()
                .and_then(|state| state.patch.map(|plan| plan.apply(effective_history)))
        } else {
            local_state.patch.map(|plan| plan.apply(effective_history))
        };

        match replacement {
            Some(replacement) => CompletionCallAction::patch(RequestPatch::new().history(replacement)),
            None if was_pruned => CompletionCallAction::patch(RequestPatch::new().history(pruned)),
            None => CompletionCallAction::continue_run(),
        }
    }
}

impl AgentEngine {
    pub(super) async fn perform_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        history: &mut Vec<Message>,
    ) -> Result<Option<crate::engine::CompactionStats>> {
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
                Ok(Some(stats))
            }
            Err(err) => {
                spinner.finish_and_clear();
                eprintln!("Warning: Proactive auto-compaction failed: {err}");
                Ok(None)
            }
        }
    }

    pub(crate) async fn check_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        (history, additional_tokens): (&mut Vec<Message>, usize),
    ) -> Result<Option<crate::engine::CompactionStats>> {
        let window = context_window(&self.config.model, &self.config.provider, &self.context);
        let tokens = trigger_tokens(
            history,
            self.usage.latest().as_ref(),
            &self.config.model,
            &self.config.provider,
            &self.context,
        );
        if should_compact(
            tokens.saturating_add(additional_tokens),
            window,
            self.config.reserve_tokens,
        ) {
            return self.perform_proactive_compaction(presenter, history).await;
        }
        Ok(None)
    }
}
#[cfg(test)]
mod tests;
