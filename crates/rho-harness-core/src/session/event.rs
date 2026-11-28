use super::SessionManager;
use super::append::Planned;
use super::format::{Durability, SessionEvent, SessionEventKind, SessionRecord, for_each_committed_record};
use super::fs::{run_blocking, session_error};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

#[derive(Deserialize)]
struct RecordType {
    record_type: String,
}

/// Reads the audit events from the committed records of the session file at `path`.
fn scan_events(path: &Path, expected_id: &str) -> Result<Vec<SessionEvent>> {
    let malformed = || session_error("session contains a malformed committed record");
    let mut events = Vec::new();
    for_each_committed_record(path, expected_id, |line| {
        let probe: RecordType = serde_json::from_slice(line).map_err(|_| malformed())?;
        if probe.record_type != "audit_event" {
            return Ok(());
        }
        match serde_json::from_slice(line).map_err(|_| malformed())? {
            SessionRecord::AuditEvent { event, .. } => events.push(event),
            _ => return Err(malformed()),
        }
        Ok(())
    })?;
    Ok(events)
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
        let session_id = self.session_id.clone();
        run_blocking(move || scan_events(&path, &session_id)).await
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

    #[test]
    fn scan_events_rejects_files_without_a_valid_header() {
        let dir = crate::test_support::temp_dir();
        let event = r#"{"record_type":"session_named","name":"x"}"#;
        let header = |version: u32, id: &str| {
            format!(
                r#"{{"record_type":"header","version":{version},"session_id":"{id}","created_at":"2020-01-02T03:04:05Z"}}"#
            )
        };
        let cases = [
            (String::new(), "mandatory version header"),
            (format!("not a header\n{event}\n"), "legacy session"),
            (
                format!("{}\n{event}\n", header(99, "bad")),
                "unsupported session version 99",
            ),
            (format!("{}\n{event}\n", header(3, "other")), "identity does not match"),
        ];
        for (body, expected) in cases {
            let path = dir.join("bad.jsonl");
            std::fs::write(&path, &body).unwrap();
            let error = scan_events(&path, "bad").unwrap_err().to_string();
            assert!(error.contains(expected), "{body:?}: {error}");
        }
    }
}
