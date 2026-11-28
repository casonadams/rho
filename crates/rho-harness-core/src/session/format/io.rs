use super::apply::{refresh_stale_messages, replay_record};
use super::types::{
    HeaderRecordType, OLDEST_SESSION_VERSION, SESSION_VERSION, SessionHeader, SessionRecord, StoreState,
};
use crate::error::{AppError, Result};
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
    if crate::fs::FSYNC_ENABLED {
        file.sync_data()?;
    }
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

#[cfg(test)]
type WriteHook = Box<dyn FnOnce() -> std::io::Result<()> + Send>;

#[cfg(test)]
static WRITE_HOOKS: std::sync::Mutex<Vec<(std::path::PathBuf, WriteHook)>> = std::sync::Mutex::new(Vec::new());

/// Runs `hook` once, after the next write to `path` has reached the file and before it is fsynced.
#[cfg(test)]
pub(crate) fn inject_write_hook(path: &Path, hook: impl FnOnce() -> std::io::Result<()> + Send + 'static) {
    WRITE_HOOKS.lock().unwrap().push((path.to_path_buf(), Box::new(hook)));
}

#[cfg(test)]
fn run_write_hook(path: &Path) -> std::io::Result<()> {
    let hook = {
        let mut hooks = WRITE_HOOKS.lock().unwrap();
        hooks
            .iter()
            .position(|(hooked, _)| hooked == path)
            .map(|i| hooks.remove(i).1)
    };
    hook.map_or(Ok(()), |hook| hook())
}

/// Why an append failed, and whether the file may still hold part of it.
#[derive(Debug)]
pub(crate) enum AppendError {
    /// The file holds nothing from this append.
    RolledBack(AppError),
    /// The file may hold some or all of this append.
    Unknown(AppError),
}

fn write_and_sync(file: &mut std::fs::File, _path: &Path, line: &[u8], durable: bool) -> std::io::Result<()> {
    file.write_all(line)?;
    #[cfg(test)]
    run_write_hook(_path)?;
    if durable && crate::fs::FSYNC_ENABLED {
        file.sync_data()?;
    }
    Ok(())
}

fn truncate_to(file: &std::fs::File, len: u64) -> std::io::Result<()> {
    file.set_len(len)?;
    if crate::fs::FSYNC_ENABLED {
        file.sync_data()?;
    }
    Ok(())
}

/// Appends `line` and returns the file length before it. A failed write is truncated away.
fn write_line_sync(path: &Path, line: &[u8], durable: bool) -> std::result::Result<u64, AppendError> {
    let rolled_back = |error: std::io::Error| AppendError::RolledBack(error.into());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
        .map_err(rolled_back)?;
    drop_torn_tail(&mut file).map_err(rolled_back)?;
    let start = file.metadata().map_err(rolled_back)?.len();
    let Err(cause) = write_and_sync(&mut file, path, line, durable) else {
        return Ok(start);
    };
    Err(match truncate_to(&file, start) {
        Ok(()) => AppendError::RolledBack(cause.into()),
        Err(undo) => AppendError::Unknown(session_error(format!(
            "{cause}; removing the partial write also failed: {undo}"
        ))),
    })
}

/// Cuts the file back to `len`, dropping records appended after it.
pub(crate) async fn truncate_file(path: &Path, len: u64) -> Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new().write(true).open(&path)?;
        truncate_to(&file, len)
    })
    .await
    .map_err(|e| session_error(format!("session truncate task failed: {e}")))?
    .map_err(Into::into)
}

/// How an appended record reaches stable storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Durability {
    /// Audit events skip the fsync; the loader drops a torn trailing line, so at most
    /// the newest unflushed events are lost on a crash.
    Audit,
    /// Canonical-history transitions are fsynced so a resumable session never replays
    /// a half-committed change.
    Durable,
}

/// Appends `records` as consecutive JSONL lines in one write and returns the file length before them.
pub(crate) async fn append_records(
    path: &Path,
    records: &[SessionRecord],
    durability: Durability,
) -> std::result::Result<u64, AppendError> {
    let mut lines = Vec::new();
    for record in records {
        lines.extend(serialize_record_line(record).map_err(AppendError::RolledBack)?);
    }
    let path = path.to_path_buf();
    let durable = durability == Durability::Durable;
    tokio::task::spawn_blocking(move || write_line_sync(&path, &lines, durable))
        .await
        .map_err(|e| AppendError::Unknown(session_error(format!("session write task failed: {e}"))))?
}

pub(crate) fn load_file(path: &Path, expected_id: &str) -> Result<StoreState> {
    parse_read_result(std::fs::read(path), expected_id)
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
    state.created_at = header.created_at;
    state.version = header.version;
    let mut messages_stale = false;
    for line in lines {
        if line.is_empty() {
            return Err(session_error("session contains an empty committed record"));
        }
        let record: SessionRecord =
            serde_json::from_slice(line).map_err(|_| session_error("session contains a malformed committed record"))?;
        replay_record(&mut state, record, expected_id, &mut messages_stale)?;
    }
    refresh_stale_messages(&mut state, &mut messages_stale);
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
    if header.version > SESSION_VERSION {
        return Err(session_error(format!(
            "unsupported session version {}: written by a newer rho; this rho reads versions {OLDEST_SESSION_VERSION} to {SESSION_VERSION}",
            header.version
        )));
    }
    if header.version < OLDEST_SESSION_VERSION {
        return Err(session_error(format!(
            "unsupported session version {}; expected version {OLDEST_SESSION_VERSION} to {SESSION_VERSION}",
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
    async fn two_managers_appending_concurrently_to_one_session_tear_or_drop_no_lines() {
        let dir = crate::test_support::temp_dir();
        let first = SessionManager::new_async(&dir, None).await.unwrap();
        let second = SessionManager::new_async(&dir, Some(&first.session_id)).await.unwrap();
        let path = first.file_path.clone();
        let payload = "x".repeat(100_000);

        let writers: Vec<_> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(who, manager)| {
                let payload = payload.clone();
                tokio::spawn(async move {
                    for n in 0..20 {
                        let event = serde_json::json!({"who": who, "n": n, "pad": payload});
                        manager
                            .append_event(SessionEventKind::UserMessage, event)
                            .await
                            .unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.await.unwrap();
        }

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.ends_with('\n'));
        let lines: Vec<&str> = log.lines().collect();
        assert!(
            lines
                .iter()
                .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
        );
        assert_eq!(lines.iter().filter(|line| line.contains("\"who\"")).count(), 40);
    }

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
            let dir = crate::test_support::temp_dir();
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
        }
    }
}
