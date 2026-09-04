use super::format::{SessionRecord, append_durable_record};
use super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::{SessionManager, session_error};
use crate::error::{AppError, Result};
use chrono::Utc;
use rig::memory::{ConversationMemory, MemoryError};
use rig::message::Message;

impl SessionManager {
    pub(crate) async fn append_messages(&self, conversation_id: &str, messages: Vec<Message>) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        if messages.is_empty() {
            return Err(session_error("canonical message batches cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        if state.checkpoint.is_some() {
            return Err(session_error(
                "pending run checkpoint must be continued before appending history",
            ));
        }
        state.integrity.check_canonical_batch(&messages)?;
        let now = Utc::now();
        let node_id = uuid::Uuid::new_v4().to_string();
        let parent_id = state.tree.active_leaf_id.clone();
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: now,
            kind: TreeNodeKind::UserTurn,
            messages: messages.clone(),
            label: None,
            metadata: None,
        };
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
        Ok(())
    }

    pub(crate) async fn clear_messages(&self, conversation_id: &str) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        let mut state = self.state.lock().await;
        let record = SessionRecord::CanonicalReset {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.messages.clear();
        state.checkpoint = None;
        state.integrity.clear();
        state.tree = SessionTree::new();
        Ok(())
    }

    pub(crate) fn ensure_conversation(&self, conversation_id: &str) -> Result<()> {
        if conversation_id != self.session_id {
            return Err(session_error(format!(
                "conversation identity mismatch: expected {}, got {conversation_id}",
                self.session_id
            )));
        }
        Ok(())
    }

    pub async fn active_messages(&self) -> Result<Vec<Message>> {
        Ok(self.state.lock().await.messages.clone())
    }

    pub(crate) fn remember_memory_error(&self, error: &AppError) {
        if let Ok(mut current) = self.memory_error.lock() {
            *current = Some(error.to_string());
        }
    }
}

impl ConversationMemory for SessionManager {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            if let Err(error) = self.ensure_conversation(conversation_id) {
                self.remember_memory_error(&error);
                return Err(MemoryError::backend(error));
            }
            Ok(self.state.lock().await.messages.clone())
        })
    }

    fn append<'a>(
        &'a self,
        conversation_id: &'a str,
        messages: Vec<Message>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            self.append_messages(conversation_id, messages).await.map_err(|error| {
                self.remember_memory_error(&error);
                MemoryError::backend(error)
            })
        })
    }

    fn clear<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            self.clear_messages(conversation_id).await.map_err(|error| {
                self.remember_memory_error(&error);
                MemoryError::backend(error)
            })
        })
    }
}
