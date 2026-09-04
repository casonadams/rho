use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use crate::error::Result;
use chrono::Utc;
use rig::message::Message;
use std::path::Path;

impl SessionManager {
    pub async fn load_tree(&self) -> Result<SessionTree> {
        Ok(self.state.lock().await.tree.clone())
    }

    pub async fn active_leaf_id(&self) -> Result<Option<String>> {
        Ok(self.state.lock().await.tree.active_leaf_id.clone())
    }

    pub async fn switch_branch(&self, leaf_id: Option<String>) -> Result<Vec<Message>> {
        let mut state = self.state.lock().await;
        state.tree.set_active_leaf(leaf_id.clone());
        let messages = state.tree.active_messages();
        state.messages = messages.clone();
        let record = SessionRecord::ActiveLeafChanged {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            active_leaf_id: leaf_id,
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(messages)
    }

    pub async fn set_node_label(&self, node_id: &str, label: Option<String>) -> Result<()> {
        let mut state = self.state.lock().await;
        state.tree.set_node_label(node_id, label.clone());
        let record = SessionRecord::SessionLabel {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            node_id: node_id.to_string(),
            label,
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(())
    }

    pub async fn set_session_name(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        state.tree.set_session_name(name.to_string());
        let record = SessionRecord::SessionNamed {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            name: name.to_string(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(())
    }

    pub async fn get_session_name(&self) -> Result<Option<String>> {
        Ok(self.state.lock().await.tree.session_name.clone())
    }

    pub fn cached_session_name(&self) -> Option<String> {
        self.state.try_lock().ok().and_then(|s| s.tree.session_name.clone())
    }

    pub async fn append_branch_summary(&self, summary: &str, source_leaf_id: &str) -> Result<()> {
        self.reject_secrets(&summary)?;
        let mut state = self.state.lock().await;
        let parent_id = state.tree.active_leaf_id.clone();
        let node_id = uuid::Uuid::new_v4().to_string();
        let summary_message = Message::assistant(format!("[Branch Summary from {source_leaf_id}]: {summary}"));
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: Utc::now(),
            kind: TreeNodeKind::BranchSummary,
            messages: vec![summary_message],
            label: Some("Branch Summary".to_string()),
            metadata: Some(serde_json::json!({ "source_leaf_id": source_leaf_id })),
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

    pub async fn fork_session(
        &self,
        sessions_dir: &Path,
        target_leaf_or_turn_id: Option<&str>,
    ) -> Result<SessionManager> {
        let tree = self.load_tree().await?;
        let target_node_id = if let Some(id_or_turn) = target_leaf_or_turn_id {
            if let Ok(turn_num) = id_or_turn.parse::<usize>() {
                let turns = self.load_turns().await?;
                if turn_num > 0 && turn_num <= turns.len() {
                    let nodes = match &tree.active_leaf_id {
                        Some(leaf) => tree.ancestor_nodes(leaf),
                        None => Vec::new(),
                    };
                    nodes.get(turn_num.saturating_sub(1)).map(|n| n.id.clone())
                } else {
                    Some(id_or_turn.to_string())
                }
            } else {
                Some(id_or_turn.to_string())
            }
        } else {
            tree.active_leaf_id.clone()
        };

        let forked = SessionManager::new(sessions_dir, None)?;
        if let Some(target_id) = target_node_id {
            let ancestors = tree.ancestor_nodes(&target_id);
            for node in ancestors {
                forked
                    .append_messages(&forked.session_id, node.messages.clone())
                    .await?;
            }
        }
        Ok(forked)
    }

    pub async fn clone_session(&self, sessions_dir: &Path) -> Result<SessionManager> {
        self.fork_session(sessions_dir, None).await
    }
}
