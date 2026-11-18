use super::super::session_error;
use super::super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::types::{SessionRecord, StoreState};
use crate::error::Result;
use crate::model::ChatMessage;

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
    messages: Vec<ChatMessage>,
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

fn apply_checkpoint_record(state: &mut StoreState, messages: Vec<ChatMessage>) -> Result<()> {
    if messages.is_empty() {
        return Err(session_error("run checkpoints cannot be empty"));
    }
    state.integrity.check_checkpoint_batch(&messages)?;
    state.checkpoint = Some(messages);
    Ok(())
}

fn apply_checkpoint_promoted(
    state: &mut StoreState,
    messages: Vec<ChatMessage>,
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

/// Applies a non-canonical record and reports whether it moved the active leaf, which leaves
/// `state.messages` stale until rebuilt from the tree.
fn apply_tree_record(state: &mut StoreState, record: SessionRecord) -> bool {
    match record {
        SessionRecord::AuditEvent { event, .. } => state.events.push(event),
        SessionRecord::TreeNode { node, .. } => {
            state.tree.add_node(node);
            state.checkpoint = None;
            return true;
        }
        SessionRecord::ActiveLeafChanged { active_leaf_id, .. } => {
            state.tree.set_active_leaf(active_leaf_id);
            return true;
        }
        SessionRecord::SessionLabel { node_id, label, .. } => {
            state.tree.set_node_label(&node_id, label);
        }
        SessionRecord::SessionNamed { name, .. } => {
            state.tree.set_session_name(name);
        }
        _ => {}
    }
    false
}

fn reset_canonical_state(state: &mut StoreState) {
    state.messages.clear();
    state.checkpoint = None;
    state.integrity.clear();
    state.tree = SessionTree::new();
}

pub fn apply_record(state: &mut StoreState, record: SessionRecord, expected_id: &str) -> Result<()> {
    let mut messages_stale = false;
    replay_record(state, record, expected_id, &mut messages_stale)?;
    refresh_stale_messages(state, &mut messages_stale);
    Ok(())
}

/// Applies one record while deferring the active-path rebuild of `state.messages` until a
/// canonical record extends it or the caller finishes replay with `refresh_stale_messages`.
pub(crate) fn replay_record(
    state: &mut StoreState,
    record: SessionRecord,
    expected_id: &str,
    messages_stale: &mut bool,
) -> Result<()> {
    let ident = record_identity_and_sequence(&record);
    let (sequence, session_id) = ident;
    validate_record_ordering(sequence, session_id, expected_id, state.next_sequence)?;
    match record {
        SessionRecord::CanonicalMessages {
            messages, timestamp, ..
        } => {
            refresh_stale_messages(state, messages_stale);
            apply_canonical_messages(state, messages, timestamp)?;
        }
        SessionRecord::CanonicalReset { .. } => {
            reset_canonical_state(state);
            *messages_stale = false;
        }
        SessionRecord::RunCheckpoint { messages, .. } => apply_checkpoint_record(state, messages)?,
        SessionRecord::CheckpointPromoted {
            messages, timestamp, ..
        } => {
            refresh_stale_messages(state, messages_stale);
            apply_checkpoint_promoted(state, messages, timestamp)?;
        }
        other => *messages_stale |= apply_tree_record(state, other),
    }
    state.next_sequence += 1;
    Ok(())
}

pub(crate) fn refresh_stale_messages(state: &mut StoreState, messages_stale: &mut bool) {
    if *messages_stale {
        state.messages = state.tree.active_messages();
        *messages_stale = false;
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::empty_store_state;
    use super::*;

    const SID: &str = "replay-session";

    fn ts() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn turn(n: usize) -> Vec<ChatMessage> {
        vec![
            ChatMessage::user(format!("u{n}")),
            ChatMessage::assistant(format!("a{n}")),
        ]
    }

    fn node(id: &str, parent: Option<&str>, messages: Vec<ChatMessage>) -> TreeNodeData {
        TreeNodeData {
            id: id.to_string(),
            parent_id: parent.map(str::to_string),
            timestamp: ts(),
            kind: TreeNodeKind::UserTurn,
            messages,
            label: None,
            metadata: None,
        }
    }

    enum Step {
        Canonical(usize),
        Node(&'static str, Option<&'static str>, usize),
        Leaf(&'static str),
        Checkpoint,
        Promote(usize),
        Reset,
    }

    fn record(step: &Step, sequence: u64) -> SessionRecord {
        let session_id = SID.to_string();
        let timestamp = ts();
        match *step {
            Step::Canonical(n) => SessionRecord::CanonicalMessages {
                sequence,
                session_id,
                timestamp,
                messages: turn(n),
            },
            Step::Node(id, parent, n) => SessionRecord::TreeNode {
                sequence,
                session_id,
                node: node(id, parent, turn(n)),
            },
            Step::Leaf(id) => SessionRecord::ActiveLeafChanged {
                sequence,
                session_id,
                timestamp,
                active_leaf_id: Some(id.to_string()),
            },
            Step::Checkpoint => SessionRecord::RunCheckpoint {
                sequence,
                session_id,
                timestamp,
                messages: vec![ChatMessage::user("u4")],
            },
            Step::Promote(n) => SessionRecord::CheckpointPromoted {
                sequence,
                session_id,
                timestamp,
                messages: turn(n),
            },
            Step::Reset => SessionRecord::CanonicalReset {
                sequence,
                session_id,
                timestamp,
            },
        }
    }

    fn records() -> Vec<SessionRecord> {
        let steps = [
            Step::Canonical(1),
            Step::Node("b", None, 2),
            Step::Canonical(3),
            Step::Leaf("b"),
            Step::Checkpoint,
            Step::Promote(4),
            Step::Node("c", Some("b"), 5),
            Step::Leaf("b"),
            Step::Reset,
            Step::Node("d", None, 6),
            Step::Canonical(7),
        ];
        steps
            .iter()
            .zip(1..)
            .map(|(step, sequence)| record(step, sequence))
            .collect()
    }

    #[test]
    fn deferred_replay_matches_eager_messages_for_every_prefix() {
        let all = records();
        for len in 0..=all.len() {
            let mut eager = empty_store_state();
            let mut deferred = empty_store_state();
            let mut stale = false;
            for record in all.iter().take(len).cloned() {
                apply_record(&mut eager, record.clone(), SID).unwrap();
                replay_record(&mut deferred, record, SID, &mut stale).unwrap();
            }
            refresh_stale_messages(&mut deferred, &mut stale);
            assert_eq!(deferred.messages, eager.messages, "prefix {len}");
            assert_eq!(deferred.messages, deferred.tree.active_messages(), "prefix {len}");
            assert_eq!(deferred.checkpoint, eager.checkpoint, "prefix {len}");
        }
    }

    #[test]
    fn replay_extends_messages_from_the_active_branch() {
        let mut state = empty_store_state();
        let mut stale = false;
        for record in records().into_iter().take(6) {
            replay_record(&mut state, record, SID, &mut stale).unwrap();
        }
        refresh_stale_messages(&mut state, &mut stale);
        let expected: Vec<ChatMessage> = turn(2).into_iter().chain(turn(4)).collect();
        assert_eq!(state.messages, expected);
        assert!(state.checkpoint.is_none());
    }
}
