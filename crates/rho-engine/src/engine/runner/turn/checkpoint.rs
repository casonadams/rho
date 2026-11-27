use super::store::TurnStore;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;

pub(super) struct TurnCheckpoint {
    pub(super) exists: bool,
    pub(super) len: usize,
}

impl TurnCheckpoint {
    pub(super) fn unsaved(&self, new_turn_messages: &[ChatMessage]) -> Vec<ChatMessage> {
        new_turn_messages[self.len..].to_vec()
    }

    pub(super) async fn persist(
        &mut self,
        store: &dyn TurnStore,
        new_turn_messages: &[ChatMessage],
    ) -> Result<(), AppError> {
        let complete = crate::engine::runner::history::without_unanswered_tool_calls(new_turn_messages);
        if complete.len() <= self.len {
            return Ok(());
        }
        store.extend_checkpoint(complete[self.len..].to_vec()).await?;
        self.exists = true;
        self.len = complete.len();
        Ok(())
    }
}

pub(super) async fn keep_partial_turn(
    store: &dyn TurnStore,
    saved: &mut TurnCheckpoint,
    new_turn_messages: &[ChatMessage],
    error: AppError,
) -> AppError {
    if !new_turn_messages
        .iter()
        .any(|m| matches!(m, ChatMessage::Assistant { .. }))
    {
        return error;
    }
    match saved.persist(store, new_turn_messages).await {
        Ok(()) => error,
        Err(save_error) => AppError::Session(format!(
            "{error}; the turn's completed work could not be kept: {save_error}"
        )),
    }
}
