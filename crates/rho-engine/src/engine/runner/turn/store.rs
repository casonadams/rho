//! Where a turn's conversation lives: the session file for the main agent, memory for a subagent.

use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::SessionManager;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// A turn's history and the id that owns its background work and messages.
///
/// A turn appends its finished steps to a checkpoint as it goes, so work survives a crash, and
/// commits the turn's messages when it completes.
#[async_trait::async_trait]
pub trait TurnStore: Send + Sync {
    /// Owns the background jobs, notices, and worktrees the turn waits for or reports.
    fn owner(&self) -> &str;
    async fn active_messages(&self) -> Result<Vec<ChatMessage>, AppError>;
    async fn has_checkpoint(&self) -> Result<bool, AppError>;
    async fn extend_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<(), AppError>;
    /// Adds the turn's remaining messages to the history, folding in the checkpoint if it has one.
    async fn commit(&self, messages: Vec<ChatMessage>, has_checkpoint: bool) -> Result<(), AppError>;
    /// Whether finished tool calls are also written to the store's event log.
    fn keeps_tool_events(&self) -> bool {
        true
    }
}

#[async_trait::async_trait]
impl TurnStore for SessionManager {
    fn owner(&self) -> &str {
        &self.session_id
    }

    async fn active_messages(&self) -> Result<Vec<ChatMessage>, AppError> {
        SessionManager::active_messages(self).await
    }

    async fn has_checkpoint(&self) -> Result<bool, AppError> {
        Ok(self.load_checkpoint().await?.is_some())
    }

    async fn extend_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<(), AppError> {
        SessionManager::extend_checkpoint(self, messages).await
    }

    async fn commit(&self, messages: Vec<ChatMessage>, has_checkpoint: bool) -> Result<(), AppError> {
        if messages.is_empty() {
            return Ok(());
        }
        if has_checkpoint {
            self.promote_checkpoint(messages).await
        } else {
            self.append_messages(&self.session_id, messages).await
        }
    }
}

#[derive(Default)]
struct Memory {
    committed: Vec<ChatMessage>,
    checkpoint: Option<Vec<ChatMessage>>,
}

/// A conversation kept in memory, for agents whose history is saved elsewhere (a subagent's task).
#[derive(Clone)]
pub struct MemoryStore {
    owner: String,
    memory: Arc<Mutex<Memory>>,
}

impl MemoryStore {
    #[must_use]
    pub fn new(owner: impl Into<String>, history: Vec<ChatMessage>) -> Self {
        Self {
            owner: owner.into(),
            memory: Arc::new(Mutex::new(Memory {
                committed: history,
                checkpoint: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Memory> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The whole conversation: committed messages, then any checkpointed steps of an unfinished turn.
    #[must_use]
    pub fn messages(&self) -> Vec<ChatMessage> {
        let memory = self.lock();
        let mut messages = memory.committed.clone();
        messages.extend(memory.checkpoint.iter().flatten().cloned());
        messages
    }
}

#[async_trait::async_trait]
impl TurnStore for MemoryStore {
    fn owner(&self) -> &str {
        &self.owner
    }

    async fn active_messages(&self) -> Result<Vec<ChatMessage>, AppError> {
        Ok(self.lock().committed.clone())
    }

    async fn has_checkpoint(&self) -> Result<bool, AppError> {
        Ok(self.lock().checkpoint.is_some())
    }

    async fn extend_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<(), AppError> {
        self.lock().checkpoint.get_or_insert_with(Vec::new).extend(messages);
        Ok(())
    }

    async fn commit(&self, messages: Vec<ChatMessage>, has_checkpoint: bool) -> Result<(), AppError> {
        let mut memory = self.lock();
        if has_checkpoint && let Some(checkpoint) = memory.checkpoint.take() {
            memory.committed.extend(checkpoint);
        }
        memory.committed.extend(messages);
        Ok(())
    }

    fn keeps_tool_events(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(messages: &[ChatMessage]) -> Vec<String> {
        messages.iter().map(|m| serde_json::to_string(m).unwrap()).collect()
    }

    #[tokio::test]
    async fn a_memory_store_keeps_history_checkpoints_and_commits_like_a_session() {
        let store = MemoryStore::new("task-1", vec![ChatMessage::user("earlier")]);
        assert_eq!(store.owner(), "task-1");
        assert!(!store.has_checkpoint().await.unwrap());

        store
            .extend_checkpoint(vec![ChatMessage::user("step 1")])
            .await
            .unwrap();
        assert!(store.has_checkpoint().await.unwrap());
        assert_eq!(
            store.active_messages().await.unwrap().len(),
            1,
            "checkpoints are not history yet"
        );
        assert_eq!(store.messages().len(), 2, "but the whole conversation includes them");

        store.commit(vec![ChatMessage::assistant("done")], true).await.unwrap();

        assert!(!store.has_checkpoint().await.unwrap());
        let expected = [
            ChatMessage::user("earlier"),
            ChatMessage::user("step 1"),
            ChatMessage::assistant("done"),
        ];
        assert_eq!(texts(&store.active_messages().await.unwrap()), texts(&expected));
        assert_eq!(texts(&store.messages()), texts(&expected));
    }
}
