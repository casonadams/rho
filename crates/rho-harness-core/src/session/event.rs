use super::SessionManager;
use super::append::Planned;
use super::format::{Durability, SessionEvent, SessionEventKind, SessionRecord};
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;
use serde_json::Value;

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
        Ok(self.state.lock().await.events.clone())
    }

    pub async fn load_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.state.lock().await.messages.clone())
    }
}
