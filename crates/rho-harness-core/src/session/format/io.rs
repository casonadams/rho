use super::apply::apply_record;
use super::types::{HeaderRecordType, SESSION_VERSION, SessionHeader, SessionRecord, StoreState};
use crate::error::Result;
use chrono::Utc;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use tokio::io::AsyncWriteExt;

use super::super::session_error;
use super::super::tree::SessionTree;
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
