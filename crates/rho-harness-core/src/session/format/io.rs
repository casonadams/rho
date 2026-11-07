use super::apply::apply_record;
use super::types::{HeaderRecordType, SESSION_VERSION, SessionHeader, SessionRecord, StoreState};
use crate::error::Result;
use chrono::Utc;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

use super::super::empty_store_state;
use super::super::session_error;

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

fn serialize_record_line(record: &SessionRecord) -> Result<Vec<u8>> {
    let mut line = serde_json::to_vec(record).map_err(|_| session_error("session record serialization failed"))?;
    line.push(b'\n');
    Ok(line)
}

fn drop_torn_tail(file: &mut std::fs::File) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let len = file.metadata()?.len();
    let mut end = len;
    let mut chunk = [0u8; 4096];
    while end > 0 {
        let start = end.saturating_sub(chunk.len() as u64);
        let buf = &mut chunk[..(end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(buf)?;
        if let Some(pos) = buf.iter().rposition(|&byte| byte == b'\n') {
            let keep = start + pos as u64 + 1;
            if keep < len {
                file.set_len(keep)?;
            }
            return Ok(());
        }
        end = start;
    }
    Ok(())
}

fn write_line_sync(path: &Path, line: &[u8], durable: bool) -> Result<()> {
    let mut file = std::fs::OpenOptions::new().read(true).append(true).open(path)?;
    drop_torn_tail(&mut file)?;
    file.write_all(line)?;
    if durable {
        file.sync_data()?;
    }
    Ok(())
}

async fn write_record(path: &Path, record: &SessionRecord, durable: bool) -> Result<()> {
    let line = serialize_record_line(record)?;
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_line_sync(&path, &line, durable))
        .await
        .map_err(|e| session_error(format!("session write task failed: {e}")))?
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
    parse_read_result(std::fs::read(path), expected_id)
}

pub async fn load_file_async(path: &Path, expected_id: &str) -> Result<StoreState> {
    parse_read_result(tokio::fs::read(path).await, expected_id)
}

fn parse_read_result(read: std::io::Result<Vec<u8>>, expected_id: &str) -> Result<StoreState> {
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(session_error(format!("unknown session id: {expected_id}")));
        }
        Err(error) => return Err(error.into()),
    };
    parse_committed_bytes(&bytes, expected_id)
}

fn parse_committed_bytes(bytes: &[u8], expected_id: &str) -> Result<StoreState> {
    let Some(last_newline) = bytes.iter().rposition(|&b| b == b'\n') else {
        return Err(session_error("session is missing the mandatory version header"));
    };
    let mut lines = bytes[..last_newline].split(|&b| b == b'\n');
    let Some(first) = lines.next() else {
        return Err(session_error("session is missing the mandatory version header"));
    };
    if first.is_empty() {
        return Err(session_error("session contains an empty committed record"));
    }
    let header = parse_header(first)?;
    validate_header(&header, expected_id)?;
    let mut state = empty_store_state();
    for line in lines {
        if line.is_empty() {
            return Err(session_error("session contains an empty committed record"));
        }
        let record: SessionRecord =
            serde_json::from_slice(line).map_err(|_| session_error("session contains a malformed committed record"))?;
        apply_record(&mut state, record, expected_id)?;
    }
    Ok(state)
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

#[cfg(test)]
mod tests {
    use super::super::super::SessionManager;
    use super::super::types::SessionEventKind;
    use std::io::Write;

    #[tokio::test]
    async fn append_after_torn_tail_keeps_session_loadable() {
        let tails = [
            (
                "partial_record",
                br#"{"record_type":"audit_event","sequence":2,"sess"#.to_vec(),
            ),
            ("tail_spanning_chunks", vec![b'x'; 5000]),
        ];
        for (name, tail) in tails {
            let dir = std::env::temp_dir().join(format!("rho_torn_tail_{}", uuid::Uuid::new_v4()));
            let first = SessionManager::new_async(&dir, None).await.unwrap();
            let session_id = first.session_id.clone();
            first
                .append_event(SessionEventKind::UserMessage, serde_json::json!({"n": 1}))
                .await
                .unwrap();
            let path = first.file_path.clone();
            drop(first);

            let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(&tail).unwrap();
            drop(file);

            let resumed = SessionManager::new_async(&dir, Some(&session_id)).await.unwrap();
            resumed
                .append_event(SessionEventKind::UserMessage, serde_json::json!({"n": 2}))
                .await
                .unwrap();
            drop(resumed);

            let reloaded = SessionManager::new_async(&dir, Some(&session_id)).await.unwrap();
            assert_eq!(reloaded.load_events().await.unwrap().len(), 2, "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
