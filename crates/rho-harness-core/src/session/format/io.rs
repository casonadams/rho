use super::types::{HeaderRecordType, SESSION_VERSION, SessionHeader, SessionRecord, StoreState};
use crate::error::Result;
use chrono::Utc;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use tokio::io::AsyncWriteExt;

use super::super::session_error;
use super::super::tree::{SessionTree, TreeNodeData, TreeNodeKind};
use super::super::validation::CanonicalHistory;

pub fn create_session_file(path: &Path, session_id: &str) -> Result<()> {
    let header = SessionHeader {
        record_type: HeaderRecordType::Header,
        version: SESSION_VERSION,
        session_id: session_id.to_string(),
        created_at: Utc::now(),
    };
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    serde_json::to_writer(&mut file, &header).map_err(|_| session_error("session header serialization failed"))?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(())
}

async fn write_record(path: &Path, record: &SessionRecord, durable: bool) -> Result<()> {
    let mut line = serde_json::to_vec(record).map_err(|_| session_error("session record serialization failed"))?;
    line.push(b'\n');
    let mut file = tokio::fs::OpenOptions::new().append(true).open(path).await?;
    file.write_all(&line).await?;
    if durable {
        // Durable boundary: full state transitions fsync; audit events do not.
        file.sync_data().await?;
    } else {
        file.flush().await?;
    }
    Ok(())
}

/// Append an audit event without an fsync; the JSONL loader drops a torn
/// trailing line, so at most the newest unflushed events are lost on a crash.
pub async fn append_record(path: &Path, record: &SessionRecord) -> Result<()> {
    write_record(path, record, false).await
}

/// Append a canonical-history state transition and fsync it so a resumable
/// session never replays a half-committed state change.
pub async fn append_durable_record(path: &Path, record: &SessionRecord) -> Result<()> {
    write_record(path, record, true).await
}

pub fn load_file(path: &Path, expected_id: &str) -> Result<StoreState> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(session_error(format!("unknown session id: {expected_id}")));
        }
        Err(error) => return Err(error.into()),
    };
    let committed = committed_lines(&bytes)?;
    let Some(first) = committed.first() else {
        return Err(session_error("session is missing the mandatory version header"));
    };
    let header = parse_header(first)?;
    validate_header(&header, expected_id)?;
    let mut state = StoreState {
        next_sequence: 1,
        messages: Vec::new(),
        checkpoint: None,
        events: Vec::new(),
        tree: SessionTree::new(),
        integrity: CanonicalHistory::new(),
    };
    for line in committed.iter().skip(1) {
        let record: SessionRecord =
            serde_json::from_slice(line).map_err(|_| session_error("session contains a malformed committed record"))?;
        apply_record(&mut state, record, expected_id)?;
    }
    Ok(state)
}

fn committed_lines(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    lines.pop();
    if lines.iter().any(|line| line.is_empty()) {
        return Err(session_error("session contains an empty committed record"));
    }
    Ok(lines)
}

pub(crate) fn parse_header(line: &[u8]) -> Result<SessionHeader> {
    let value: Value = serde_json::from_slice(line).map_err(|_| session_error("legacy session cannot be resumed"))?;
    if value.get("record_type").and_then(Value::as_str) != Some("header") {
        return Err(session_error("legacy session cannot be resumed"));
    }
    if value.get("version").is_none() {
        return Err(session_error("session is missing the mandatory version header"));
    }
    serde_json::from_value(value).map_err(|_| session_error("session version header is malformed"))
}

pub(crate) fn validate_header(header: &SessionHeader, expected_id: &str) -> Result<()> {
    if header.version != SESSION_VERSION {
        return Err(session_error(format!(
            "unsupported session version {}; expected version {SESSION_VERSION}",
            header.version
        )));
    }
    if header.session_id != expected_id {
        return Err(session_error("session identity does not match its file name"));
    }
    Ok(())
}

pub(crate) fn apply_record(state: &mut StoreState, record: SessionRecord, expected_id: &str) -> Result<()> {
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
