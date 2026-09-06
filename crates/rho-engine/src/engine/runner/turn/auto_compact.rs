use crate::engine::AgentEngine;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::presentation::presenter::Presenter;
use rig::memory::ConversationMemory;
use rig::message::Message;

impl AgentEngine {
    pub(super) async fn perform_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        history: &mut Vec<Message>,
    ) -> Result<()> {
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

    pub(super) async fn check_proactive_compaction(
        &self,
        presenter: &dyn Presenter,
        history: &mut Vec<Message>,
    ) -> Result<()> {
        let window = self
            .context_limit()
            .unwrap_or_else(|| rho_harness_core::tokens::context_window_size(&self.config.model));
        let estimated =
            rho_harness_core::tokens::calculate_context_tokens(history, None, &self.config.model).total_tokens;
        let consumed = self
            .usage
            .latest()
            .map(|u| {
                (u.input_tokens + u.cached_input_tokens.unwrap_or(0) + u.cache_creation_input_tokens.unwrap_or(0))
                    as usize
            })
            .unwrap_or(0);
        let tokens = estimated.max(consumed);
        if rho_harness_core::tokens::should_compact(tokens, window, self.config.reserve_tokens) {
            self.perform_proactive_compaction(presenter, history).await?;
        }
        Ok(())
    }
}
