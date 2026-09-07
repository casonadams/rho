use super::super::session_error;
use super::super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::types::{SessionRecord, StoreState};
use crate::error::Result;

macro_rules! extract_record_ident {
    ($record:expr) => {
        match $record {
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
            } => (*sequence, session_id.as_str()),
        }
    };
}

fn record_identity_and_sequence(record: &SessionRecord) -> (u64, &str) {
    extract_record_ident!(record)
}

fn validate_record_ordering(sequence: u64, session_id: &str, expected_id: &str, next_seq: u64) -> Result<()> {
    if session_id != expected_id {
        return Err(session_error("session record identity mismatch"));
    }
    if sequence != next_seq {
        return Err(session_error("session record ordering is invalid"));
    }
    Ok(())
}

fn apply_canonical_messages(
    state: &mut StoreState,
    messages: Vec<rig::message::Message>,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    if messages.is_empty() {
        return Err(session_error("canonical message batches cannot be empty"));
    }
    state.integrity.check_canonical_batch(&messages)?;
    state.messages.extend(messages.clone());
    let node = TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id: state.tree.active_leaf_id.clone(),
        timestamp,
        kind: TreeNodeKind::UserTurn,
        messages,
        label: None,
        metadata: None,
    };
    state.tree.add_node(node);
    Ok(())
}

fn apply_checkpoint_record(state: &mut StoreState, messages: Vec<rig::message::Message>) -> Result<()> {
    if messages.is_empty() {
        return Err(session_error("run checkpoints cannot be empty"));
    }
    state.integrity.check_checkpoint_batch(&messages)?;
    state.checkpoint = Some(messages);
    Ok(())
}

fn apply_checkpoint_promoted(
    state: &mut StoreState,
    messages: Vec<rig::message::Message>,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
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
    let node = TreeNodeData {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id: state.tree.active_leaf_id.clone(),
        timestamp,
        kind: TreeNodeKind::AssistantTurn,
        messages,
        label: None,
        metadata: None,
    };
    state.tree.add_node(node);
    Ok(())
}

fn apply_tree_record(state: &mut StoreState, record: SessionRecord) {
    match record {
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
        _ => {}
    }
}

fn reset_canonical_state(state: &mut StoreState) {
    state.messages.clear();
    state.checkpoint = None;
    state.integrity.clear();
    state.tree = SessionTree::new();
}

pub fn apply_record(state: &mut StoreState, record: SessionRecord, expected_id: &str) -> Result<()> {
    let ident = record_identity_and_sequence(&record);
    let (sequence, session_id) = ident;
    validate_record_ordering(sequence, session_id, expected_id, state.next_sequence)?;
    match record {
        SessionRecord::CanonicalMessages {
            messages, timestamp, ..
        } => {
            apply_canonical_messages(state, messages, timestamp)?;
        }
        SessionRecord::CanonicalReset { .. } => reset_canonical_state(state),
        SessionRecord::RunCheckpoint { messages, .. } => apply_checkpoint_record(state, messages)?,
        SessionRecord::CheckpointPromoted {
            messages, timestamp, ..
        } => {
            apply_checkpoint_promoted(state, messages, timestamp)?;
        }
        other => apply_tree_record(state, other),
    }
    state.next_sequence += 1;
    Ok(())
}
