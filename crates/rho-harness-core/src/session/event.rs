use super::SessionManager;
use super::append::Planned;
use super::format::{Durability, SessionEvent, SessionEventKind, SessionRecord};
use super::fs::{run_blocking, session_error};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::io::BufRead;
use std::path::Path;

#[derive(Deserialize)]
struct RecordType {
    record_type: String,
}

/// Reads the audit events from the committed records after the header; a torn trailing line is ignored.
fn scan_events(path: &Path) -> Result<Vec<SessionEvent>> {
    let malformed = || session_error("session contains a malformed committed record");
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    let mut events = Vec::new();
    loop {
        line.clear();
        reader.read_until(b'\n', &mut line)?;
        if line.pop() != Some(b'\n') {
            return Ok(events);
        }
        let probe: RecordType = serde_json::from_slice(&line).map_err(|_| malformed())?;
        if probe.record_type != "audit_event" {
            continue;
        }
        match serde_json::from_slice(&line).map_err(|_| malformed())? {
            SessionRecord::AuditEvent { event, .. } => events.push(event),
            _ => return Err(malformed()),
        }
    }
}

impl SessionManager {
    pub async fn append_event(&self, kind: SessionEventKind, payload: Value) -> Result<()> {
        self.reject_secrets(&payload)?;
        let event = SessionEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            kind,
            payload,
        };
        self.append(Durability::Audit, move |state, session_id| {
            let record = SessionRecord::AuditEvent {
                sequence: state.next_sequence,
                session_id: session_id.to_string(),
                event,
            };
            Ok(Planned {
                records: vec![record],
                output: (),
            })
        })
        .await
    }

    pub async fn load_events(&self) -> Result<Vec<SessionEvent>> {
        let _writers_held = self.state.lock().await;
        let path = self.file_path.clone();
        run_blocking(move || scan_events(&path)).await
    }

    pub async fn load_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.state.lock().await.messages.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KINDS: [SessionEventKind; 3] = [
        SessionEventKind::ToolCall,
        SessionEventKind::ToolResult,
        SessionEventKind::UsageMetrics,
    ];

    #[tokio::test]
    async fn load_events_returns_every_committed_event_in_order_live_and_after_reopening() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".into()]).unwrap();
        let id = session.session_id.clone();
        let mut expected = Vec::new();
        for n in 0..300 {
            let kind = KINDS[n % KINDS.len()];
            session.append_event(kind, json!({ "n": n })).await.unwrap();
            expected.push((kind, json!({ "n": n })));
            if n % 50 == 0 {
                let turn = vec![ChatMessage::user(format!("q{n}")), ChatMessage::assistant("a")];
                session.append_messages(&id, turn).await.unwrap();
                session.clear_messages(&id).await.unwrap();
            }
        }
        let rejected = json!({ "text": "credential-sentinel" });
        assert!(
            session
                .append_event(SessionEventKind::UserMessage, rejected)
                .await
                .is_err()
        );

        let live = session.load_events().await.unwrap();
        let seen: Vec<_> = live.iter().map(|e| (e.kind, e.payload.clone())).collect();
        assert_eq!(seen, expected);
        drop(session);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(reopened.load_events().await.unwrap(), live);
    }
}
