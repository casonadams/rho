use super::append::{Planned, tree_node};
use super::format::{Durability, SessionRecord, StoreState};
use super::tree::{TreeNodeData, TreeNodeKind};
use super::{SessionManager, session_error};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;

impl SessionManager {
    pub async fn append_messages(&self, conversation_id: &str, messages: Vec<ChatMessage>) -> Result<()> {
        self.append_message_batches(conversation_id, vec![messages]).await
    }

    pub(crate) async fn append_message_batches(
        &self,
        conversation_id: &str,
        batches: Vec<Vec<ChatMessage>>,
    ) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        if batches.iter().any(Vec::is_empty) {
            return Err(session_error("canonical message batches cannot be empty"));
        }
        self.reject_secrets(&batches)?;
        self.append(Durability::Durable, move |state, session_id| {
            if state.checkpoint.is_some() {
                return Err(session_error(
                    "pending run checkpoint must be continued before appending history",
                ));
            }
            Ok(Planned {
                records: plan_batch_nodes(state, session_id, batches)?,
                output: (),
            })
        })
        .await
    }

    pub async fn clear_messages(&self, conversation_id: &str) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        self.append(Durability::Durable, |state, session_id| {
            let record = SessionRecord::CanonicalReset {
                sequence: state.next_sequence,
                session_id: session_id.to_string(),
                timestamp: Utc::now(),
            };
            Ok(Planned {
                records: vec![record],
                output: (),
            })
        })
        .await
    }

    pub fn ensure_conversation(&self, conversation_id: &str) -> Result<()> {
        if conversation_id != self.session_id {
            return Err(session_error(format!(
                "conversation identity mismatch: expected {}, got {conversation_id}",
                self.session_id
            )));
        }
        Ok(())
    }

    pub async fn active_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.state.lock().await.messages.clone())
    }
}

/// Chains one user-turn node per batch onto the active leaf, one record each.
fn plan_batch_nodes(
    state: &StoreState,
    session_id: &str,
    batches: Vec<Vec<ChatMessage>>,
) -> Result<Vec<SessionRecord>> {
    let mut parent_id = state.tree.active_leaf_id.clone();
    let mut records = Vec::with_capacity(batches.len());
    for messages in batches {
        state.integrity.check_canonical_batch(&messages)?;
        let node = TreeNodeData::turn(TreeNodeKind::UserTurn, parent_id, Utc::now(), messages);
        parent_id = Some(node.id.clone());
        records.push(tree_node(state.next_sequence + records.len() as u64, session_id, node));
    }
    Ok(records)
}
