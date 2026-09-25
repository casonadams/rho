use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use crate::error::Result;
use chrono::Utc;
use rig::message::Message;
use std::path::Path;

fn create_branch_summary_node(parent_id: Option<String>, source_leaf_id: &str, summary: &str) -> TreeNodeData {
    let summary_message = Message::assistant(format!("[Branch Summary from {source_leaf_id}]: {summary}"));
    TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id,
        timestamp: Utc::now(),
        kind: TreeNodeKind::BranchSummary,
        messages: vec![summary_message],
        label: Some("Branch Summary".to_string()),
        metadata: Some(serde_json::json!({ "source_leaf_id": source_leaf_id })),
    }
}

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
        let node = create_branch_summary_node(state.tree.active_leaf_id.clone(), source_leaf_id, summary);
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

    async fn resolve_turn_node_id(
        &self,
        tree: &SessionTree,
        turn_num: usize,
        id_or_turn: &str,
    ) -> Result<Option<String>> {
        let turns = self.load_turns().await?;
        Ok(resolve_turn_node_id_from_tree(tree, turn_num, turns.len(), id_or_turn))
    }

    async fn resolve_target_node_id(&self, tree: &SessionTree, target: Option<&str>) -> Result<Option<String>> {
        let Some(id_or_turn) = target else {
            return Ok(tree.active_leaf_id.clone());
        };
        if let Ok(turn_num) = id_or_turn.parse::<usize>() {
            self.resolve_turn_node_id(tree, turn_num, id_or_turn).await
        } else {
            Ok(Some(id_or_turn.to_string()))
        }
    }

    async fn copy_ancestors_to_forked(
        forked: &SessionManager,
        tree: &SessionTree,
        target_id: Option<String>,
    ) -> Result<()> {
        let Some(target_id) = target_id else {
            return Ok(());
        };
        for node in tree.ancestor_nodes(&target_id) {
            forked
                .append_messages(&forked.session_id, node.messages.clone())
                .await?;
        }
        Ok(())
    }

    pub async fn fork_session(
        &self,
        sessions_dir: &Path,
        target_leaf_or_turn_id: Option<&str>,
    ) -> Result<SessionManager> {
        let tree = self.load_tree().await?;
        let target_id = self.resolve_target_node_id(&tree, target_leaf_or_turn_id).await?;
        let forked = SessionManager::new_async(sessions_dir, None).await?;
        Self::copy_ancestors_to_forked(&forked, &tree, target_id).await?;
        Ok(forked)
    }

    pub async fn clone_session(&self, sessions_dir: &Path) -> Result<SessionManager> {
        self.fork_session(sessions_dir, None).await
    }
}

fn resolve_turn_node_id_from_tree(
    tree: &SessionTree,
    turn_num: usize,
    turn_count: usize,
    fallback_id: &str,
) -> Option<String> {
    if turn_num > 0 && turn_num <= turn_count {
        let nodes = match &tree.active_leaf_id {
            Some(leaf) => tree.ancestor_nodes(leaf),
            None => Vec::new(),
        };
        nodes.get(turn_num.saturating_sub(1)).map(|n| n.id.clone())
    } else {
        Some(fallback_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tree::{TreeNodeData, TreeNodeKind};
    use rig::message::Message;

    fn create_test_node(id: &str, parent_id: Option<&str>) -> TreeNodeData {
        TreeNodeData {
            id: id.to_string(),
            parent_id: parent_id.map(|p| p.to_string()),
            timestamp: chrono::Utc::now(),
            kind: TreeNodeKind::UserTurn,
            messages: Vec::new(),
            label: None,
            metadata: None,
        }
    }

    #[test]
    fn test_resolve_turn_node_id_from_tree() {
        let mut tree = SessionTree::default();
        let node1 = create_test_node("node-1", None);
        let node2 = create_test_node("node-2", Some("node-1"));
        tree.add_node(node1);
        tree.add_node(node2);
        tree.active_leaf_id = Some("node-2".to_string());

        assert_eq!(
            resolve_turn_node_id_from_tree(&tree, 1, 2, "fallback"),
            Some("node-1".to_string())
        );
        assert_eq!(
            resolve_turn_node_id_from_tree(&tree, 2, 2, "fallback"),
            Some("node-2".to_string())
        );
        assert_eq!(
            resolve_turn_node_id_from_tree(&tree, 0, 2, "fallback"),
            Some("fallback".to_string())
        );
        assert_eq!(
            resolve_turn_node_id_from_tree(&tree, 3, 2, "fallback"),
            Some("fallback".to_string())
        );

        let empty_tree = SessionTree::default();
        assert_eq!(resolve_turn_node_id_from_tree(&empty_tree, 1, 2, "fallback"), None);
    }

    #[tokio::test]
    async fn test_fork_and_clone_session() {
        let dir = std::env::temp_dir().join(format!("session_fork_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session = SessionManager::new(&dir, None).unwrap();
        let sid = session.session_id.clone();
        let m1 = Message::user("hello");
        let m2 = Message::assistant("world");
        session.append_messages(&sid, vec![m1, m2]).await.unwrap();

        let forked_turn = session.fork_session(&dir, Some("1")).await.unwrap();
        assert_ne!(forked_turn.session_id, session.session_id);
        let msgs = forked_turn.load_messages().await.unwrap();
        assert_eq!(msgs.len(), 2);

        let tree = session.load_tree().await.unwrap();
        let leaf = tree.active_leaf_id.as_deref().unwrap();
        let forked_leaf = session.fork_session(&dir, Some(leaf)).await.unwrap();
        assert_eq!(forked_leaf.load_messages().await.unwrap().len(), 2);

        let cloned = session.clone_session(&dir).await.unwrap();
        assert_ne!(cloned.session_id, session.session_id);
        assert_eq!(cloned.load_messages().await.unwrap().len(), 2);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
