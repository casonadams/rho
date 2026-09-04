use super::SessionManager;
use super::format::{SessionEvent, SessionEventKind, SessionRecord, append_record};
use crate::error::Result;
use chrono::Utc;
use rig::message::Message;
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
        let mut state = self.state.lock().await;
        let record = SessionRecord::AuditEvent {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            event: event.clone(),
        };
        append_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.events.push(event);
        Ok(())
    }

    pub async fn load_events(&self) -> Result<Vec<SessionEvent>> {
        Ok(self.state.lock().await.events.clone())
    }

    pub async fn load_messages(&self) -> Result<Vec<Message>> {
        Ok(self.state.lock().await.messages.clone())
    }
}
