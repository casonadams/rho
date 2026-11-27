use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use super::fs::session_error;
use super::tree::{TreeNodeData, TreeNodeKind};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;

impl SessionManager {
    pub async fn load_checkpoint(&self) -> Result<Option<Vec<ChatMessage>>> {
        Ok(self.state.lock().await.checkpoint.clone())
    }

    pub async fn save_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<()> {
        if messages.is_empty() {
            return Err(session_error("run checkpoints cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        state.integrity.check_checkpoint_batch(&messages)?;
        let record = SessionRecord::RunCheckpoint {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            messages: messages.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.checkpoint = Some(messages);
        Ok(())
    }

    pub async fn extend_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<()> {
        if messages.is_empty() {
            return Err(session_error("run checkpoint extensions cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        state.integrity.check_checkpoint_batch(&messages)?;
        let (sequence, session_id, timestamp) = (state.next_sequence, self.session_id.clone(), Utc::now());
        let record = if state.checkpoint.is_some() {
            SessionRecord::RunCheckpointExtended {
                sequence,
                session_id,
                timestamp,
                messages: messages.clone(),
            }
        } else {
            SessionRecord::RunCheckpoint {
                sequence,
                session_id,
                timestamp,
                messages: messages.clone(),
            }
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.checkpoint.get_or_insert_with(Vec::new).extend(messages);
        Ok(())
    }

    pub async fn promote_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<()> {
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        let mut promoted = state
            .checkpoint
            .clone()
            .ok_or_else(|| session_error("run checkpoint is missing"))?;
        promoted.extend(messages);
        state.integrity.check_canonical_batch(&promoted)?;

        let node = TreeNodeData::turn(
            TreeNodeKind::AssistantTurn,
            state.tree.active_leaf_id.clone(),
            Utc::now(),
            promoted,
        );
        self.append_tree_node(&mut state, node).await?;
        state.checkpoint = None;
        Ok(())
    }
}
