use super::SessionManager;
use super::append::{Planned, tree_node};
use super::format::{Durability, SessionRecord};
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
            Ok(Planned {
                records: vec![tree_node(state.next_sequence, session_id, node)],
                output: (),
            })
        })
        .await
    }
}
