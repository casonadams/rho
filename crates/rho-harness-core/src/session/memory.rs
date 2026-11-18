use super::format::{SessionRecord, append_durable_record, append_durable_records};
use super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::{SessionManager, session_error};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;

fn create_user_turn_node(parent_id: Option<String>, messages: Vec<ChatMessage>) -> TreeNodeData {
    TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages,
        label: None,
        metadata: None,
    }
}

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
        let mut state = self.state.lock().await;
        if state.checkpoint.is_some() {
            return Err(session_error(
                "pending run checkpoint must be continued before appending history",
            ));
        }
        if batches.is_empty() {
            return Ok(());
        }
        let mut parent_id = state.tree.active_leaf_id.clone();
        let mut nodes = Vec::with_capacity(batches.len());
        for messages in batches {
            state.integrity.check_canonical_batch(&messages)?;
            let node = create_user_turn_node(parent_id, messages);
            parent_id = Some(node.id.clone());
            nodes.push(node);
        }
        let records: Vec<SessionRecord> = nodes
            .iter()
            .zip(state.next_sequence..)
            .map(|(node, sequence)| SessionRecord::TreeNode {
                sequence,
                session_id: self.session_id.clone(),
                node: node.clone(),
            })
            .collect();
        append_durable_records(&self.file_path, &records).await?;
        for node in nodes {
            state.next_sequence += 1;
            state.tree.add_node(node);
        }
        state.messages = state.tree.active_messages();
        Ok(())
    }

    pub async fn clear_messages(&self, conversation_id: &str) -> Result<()> {
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
