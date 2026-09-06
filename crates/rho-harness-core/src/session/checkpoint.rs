use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use super::fs::session_error;
use super::tree::{TreeNodeData, TreeNodeKind};
use crate::error::Result;
use chrono::Utc;
use rig::message::Message;

fn create_turn_node(parent_id: Option<String>, messages: Vec<Message>) -> TreeNodeData {
    TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id,
        timestamp: Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages,
        label: None,
        metadata: None,
    }
}

impl SessionManager {
    pub async fn load_checkpoint(&self) -> Result<Option<Vec<Message>>> {
        Ok(self.state.lock().await.checkpoint.clone())
    }

    pub async fn save_checkpoint(&self, messages: Vec<Message>) -> Result<()> {
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

    pub async fn promote_checkpoint(&self, messages: Vec<Message>) -> Result<()> {
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        let mut promoted = state
            .checkpoint
            .clone()
            .ok_or_else(|| session_error("run checkpoint is missing"))?;
        promoted.extend(messages);
        state.integrity.check_canonical_batch(&promoted)?;

        let node = create_turn_node(state.tree.active_leaf_id.clone(), promoted);
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
        state.checkpoint = None;
        Ok(())
    }
}
