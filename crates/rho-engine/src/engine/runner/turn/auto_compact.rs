use std::sync::{Arc, Mutex};

use crate::engine::AgentEngine;
use crate::engine::compactor::SessionCompactor;
use crate::engine::compactor::llm::{LlmCompactor, SummarizeOptions};
use crate::engine::metrics::StructuralUsage;
use crate::engine::tracking::{ContextTracker, UsageTracker};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::session::compaction::compaction_summary_message;
use rho_harness_core::tokens::{
    calculate_context_tokens, context_window_size_for_provider, estimate_message_tokens, find_token_cut_point,
    should_compact,
};
use rig::agent::hook::{AgentHook, CompletionCall, CompletionCallAction, HookContext, RequestPatch};
use rig::memory::ConversationMemory;
use rig::message::Message;

/// Provider-anchored context size: input, output, and cache reads/writes of the
/// latest model call.
fn usage_anchor_tokens(usage: &StructuralUsage) -> usize {
    (usage.input_tokens
        + usage.output_tokens
        + usage.cached_input_tokens.unwrap_or(0)
        + usage.cache_creation_input_tokens.unwrap_or(0)) as usize
}

fn estimated_tokens(messages: &[Message], model: &str) -> usize {
    calculate_context_tokens(messages, None, model).total_tokens
}

/// Compaction pressure: the more reliable of the provider-reported usage anchor
/// and a tokenizer estimate over the messages themselves.
fn trigger_tokens(messages: &[Message], usage: Option<&StructuralUsage>, model: &str) -> usize {
    estimated_tokens(messages, model).max(usage.map(usage_anchor_tokens).unwrap_or(0))
}

fn context_window(model: &str, provider: &str, context: ContextTracker) -> usize {
    context
        .limit_for(model, provider)
        .unwrap_or_else(|| context_window_size_for_provider(model, provider))
}

/// Replacement history for a compacted model call: `prefix` stands in for
/// `history[..cut]`, and everything from `cut` on is kept verbatim.
#[derive(Debug, Clone)]
struct PatchPlan {
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

#[derive(Default)]
struct HookState {
    base_len: Option<usize>,
    tripped: bool,
    patch: Option<PatchPlan>,
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
    state: Mutex<HookState>,
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
        Self {
            compactor,
            presenter,
            usage,
            context,
            provider: provider.to_string(),
            reserve_tokens,
            state: Mutex::new(HookState::default()),
        }
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
        let compactor = LlmCompactor::new(self.compactor.model().cloned());
        let summary = compactor
            .summarize(
                &history[..cut.cut_index],
                SummarizeOptions {
                    prior_summary: None,
                    custom_instructions: None,
                    is_split_turn: cut.is_split_turn,
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
        let tokens_after = calculate_context_tokens(&kept_with_summary, None, model).total_tokens;
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
    async fn on_completion_call(&self, _ctx: &HookContext, event: CompletionCall<'_>) -> CompletionCallAction {
        self.handle(event.history, event.prompt).await
    }
}

impl AutoCompactHook {
    async fn handle(&self, history: &[Message], prompt: &Message) -> CompletionCallAction {
        let tripped = {
            let mut state = self.state.lock().unwrap();
            state.base_len.get_or_insert(history.len());
            state.tripped
        };
        if !tripped {
            let window = context_window(self.model_name(), &self.provider, self.context);
            let mut messages = estimated_tokens(history, self.model_name());
            messages = messages
                .saturating_add(estimate_message_tokens(prompt, self.model_name()))
                .max(self.usage.latest().map(|u| usage_anchor_tokens(&u)).unwrap_or(0));
            if should_compact(messages, window, self.reserve_tokens) {
                let base_len = self.state.lock().unwrap().base_len.unwrap_or(history.len());
                let plan = self.compact_and_plan(history, base_len).await;
                let mut state = self.state.lock().unwrap();
                state.tripped = true;
                state.patch = plan;
            }
        }
        let replacement = {
            let state = self.state.lock().unwrap();
            state.patch.as_ref().map(|plan| plan.apply(history))
        };
        match replacement {
            Some(replacement) => CompletionCallAction::patch(RequestPatch::new().history(replacement)),
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
        let window = context_window(&self.config.model, &self.config.provider, self.context);
        let tokens = trigger_tokens(history, self.usage.latest().as_ref(), &self.config.model);
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
