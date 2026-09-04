use super::super::session_error;
use super::super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::types::{SessionRecord, StoreState};
use crate::error::Result;

pub fn apply_record(state: &mut StoreState, record: SessionRecord, expected_id: &str) -> Result<()> {
    let (sequence, session_id) = match &record {
        SessionRecord::CanonicalMessages {
            sequence, session_id, ..
        }
        | SessionRecord::CanonicalReset {
            sequence, session_id, ..
        }
        | SessionRecord::RunCheckpoint {
            sequence, session_id, ..
        }
        | SessionRecord::CheckpointPromoted {
            sequence, session_id, ..
        }
        | SessionRecord::AuditEvent {
            sequence, session_id, ..
        }
        | SessionRecord::TreeNode {
            sequence, session_id, ..
        }
        | SessionRecord::ActiveLeafChanged {
            sequence, session_id, ..
        }
        | SessionRecord::SessionLabel {
            sequence, session_id, ..
        }
        | SessionRecord::SessionNamed {
            sequence, session_id, ..
        } => (*sequence, session_id),
    };
    if session_id != expected_id {
        return Err(session_error("session record identity mismatch"));
    }
    if sequence != state.next_sequence {
        return Err(session_error("session record ordering is invalid"));
    }
    match record {
        SessionRecord::CanonicalMessages {
            messages, timestamp, ..
        } => {
            if messages.is_empty() {
                return Err(session_error("canonical message batches cannot be empty"));
            }
            state.integrity.check_canonical_batch(&messages)?;
            state.messages.extend(messages.clone());
            let node_id = uuid::Uuid::new_v4().to_string();
            let parent_id = state.tree.active_leaf_id.clone();
            state.tree.add_node(TreeNodeData {
                id: node_id,
                parent_id,
                timestamp,
                kind: TreeNodeKind::UserTurn,
                messages,
                label: None,
                metadata: None,
            });
        }
        SessionRecord::CanonicalReset { .. } => {
            state.messages.clear();
            state.checkpoint = None;
            state.integrity.clear();
            state.tree = SessionTree::new();
        }
        SessionRecord::RunCheckpoint { messages, .. } => {
            if messages.is_empty() {
                return Err(session_error("run checkpoints cannot be empty"));
            }
            state.integrity.check_checkpoint_batch(&messages)?;
            state.checkpoint = Some(messages);
        }
        SessionRecord::CheckpointPromoted {
            messages, timestamp, ..
        } => {
            let checkpoint = state
                .checkpoint
                .as_ref()
                .ok_or_else(|| session_error("checkpoint promotion ordering is invalid"))?;
            if messages.is_empty() || !messages.starts_with(checkpoint) {
                return Err(session_error("checkpoint promotion does not match pending history"));
            }
            state.integrity.check_canonical_batch(&messages)?;
            state.messages.extend(messages.clone());
            state.checkpoint = None;
            let node_id = uuid::Uuid::new_v4().to_string();
            let parent_id = state.tree.active_leaf_id.clone();
            state.tree.add_node(TreeNodeData {
                id: node_id,
                parent_id,
                timestamp,
                kind: TreeNodeKind::AssistantTurn,
                messages,
                label: None,
                metadata: None,
            });
        }
        SessionRecord::AuditEvent { event, .. } => state.events.push(event),
        SessionRecord::TreeNode { node, .. } => {
            state.tree.add_node(node);
            state.messages = state.tree.active_messages();
            state.checkpoint = None;
        }
        SessionRecord::ActiveLeafChanged { active_leaf_id, .. } => {
            state.tree.set_active_leaf(active_leaf_id);
            state.messages = state.tree.active_messages();
        }
        SessionRecord::SessionLabel { node_id, label, .. } => {
            state.tree.set_node_label(&node_id, label);
        }
        SessionRecord::SessionNamed { name, .. } => {
            state.tree.set_session_name(name);
        }
    }
    state.next_sequence += 1;
    Ok(())
}
