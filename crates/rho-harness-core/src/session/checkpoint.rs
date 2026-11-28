use super::SessionManager;
use super::append::Planned;
use super::format::{Durability, SessionRecord, StoreState};
use super::fs::session_error;
use super::tree::TreeNodeKind;
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
        self.append(Durability::Durable, move |state, session_id| {
            state.integrity.check_checkpoint_batch(&messages)?;
            let record = SessionRecord::RunCheckpoint {
                sequence: state.next_sequence,
                session_id: session_id.to_string(),
                timestamp: Utc::now(),
                messages,
            };
            Ok(Planned {
                records: vec![record],
                output: (),
            })
        })
        .await
    }

    pub async fn extend_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<()> {
        if messages.is_empty() {
            return Err(session_error("run checkpoint extensions cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        self.append(Durability::Durable, move |state, session_id| {
            state.integrity.check_checkpoint_batch(&messages)?;
            let (sequence, session_id, timestamp) = (state.next_sequence, session_id.to_string(), Utc::now());
            let record = if state.checkpoint.is_some() {
                SessionRecord::RunCheckpointExtended {
                    sequence,
                    session_id,
                    timestamp,
                    messages,
                }
            } else {
                SessionRecord::RunCheckpoint {
                    sequence,
                    session_id,
                    timestamp,
                    messages,
                }
            };
            Ok(Planned {
                records: vec![record],
                output: (),
            })
        })
        .await
    }

    pub async fn promote_checkpoint(&self, messages: Vec<ChatMessage>) -> Result<()> {
        self.reject_secrets(&messages)?;
        self.append(Durability::Durable, move |state, session_id| {
            let checkpoint = state
                .checkpoint
                .as_ref()
                .ok_or_else(|| session_error("run checkpoint is missing"))?;
            let record = promotion_record(state, session_id, checkpoint, messages)?;
            Ok(Planned {
                records: vec![record],
                output: (),
            })
        })
        .await
    }
}

/// The messages that close a pending checkpoint: `note`, unless it already ends with an assistant message.
pub fn unfinished_turn_closing(checkpoint: &[ChatMessage], note: ChatMessage) -> Vec<ChatMessage> {
    match checkpoint.last() {
        Some(ChatMessage::Assistant { .. }) => Vec::new(),
        _ => vec![note],
    }
}

/// The record that makes `checkpoint` followed by `closing` a turn under the active leaf.
pub(crate) fn promotion_record(
    state: &StoreState,
    session_id: &str,
    checkpoint: &[ChatMessage],
    closing: Vec<ChatMessage>,
) -> Result<SessionRecord> {
    let promoted: Vec<ChatMessage> = checkpoint.iter().chain(&closing).cloned().collect();
    state.integrity.check_canonical_batch(&promoted)?;
    Ok(SessionRecord::CheckpointNode {
        sequence: state.next_sequence,
        session_id: session_id.to_string(),
        timestamp: Utc::now(),
        node_id: uuid::Uuid::new_v4().to_string(),
        parent_id: state.tree.active_leaf_id.clone(),
        kind: TreeNodeKind::AssistantTurn,
        messages: closing,
    })
}
