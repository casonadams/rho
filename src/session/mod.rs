pub mod context;

use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use rig::memory::{ConversationMemory, MemoryError};
use rig::message::{AssistantContent, Message, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SESSION_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct SessionHeader {
    record_type: HeaderRecordType,
    version: u32,
    session_id: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HeaderRecordType {
    Header,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "record_type", rename_all = "snake_case", deny_unknown_fields)]
enum SessionRecord {
    CanonicalMessages {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    CanonicalReset {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
    },
    RunCheckpoint {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    CheckpointPromoted {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    AuditEvent {
        sequence: u64,
        session_id: String,
        event: SessionEvent,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub kind: SessionEventKind,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    SystemPrompt,
    UserMessage,
    Reasoning,
    AssistantResponse,
    ToolCall,
    ToolResult,
    UsageMetrics,
    RunSummary,
    Cancellation,
    RunFailed,
}

#[derive(Debug)]
struct StoreState {
    next_sequence: u64,
    messages: Vec<Message>,
    checkpoint: Option<Vec<Message>>,
    events: Vec<SessionEvent>,
}

#[derive(Clone, Debug)]
pub struct SessionManager {
    pub session_id: String,
    pub file_path: PathBuf,
    state: Arc<tokio::sync::Mutex<StoreState>>,
    secrets: Arc<Mutex<Vec<String>>>,
    memory_error: Arc<Mutex<Option<String>>>,
}

impl SessionManager {
    pub fn new(sessions_dir: &Path, resume_id: Option<&str>) -> Result<Self> {
        Self::new_with_secrets(sessions_dir, resume_id, Vec::new())
    }

    pub fn new_with_secrets(sessions_dir: &Path, resume_id: Option<&str>, secrets: Vec<String>) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)?;
        let session_id = resume_id.map_or_else(new_session_id, str::to_string);
        validate_session_id(&session_id)?;
        let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
        let state = match resume_id {
            Some(_) => load_file(&file_path, &session_id)?,
            None => {
                create_session_file(&file_path, &session_id)?;
                StoreState {
                    next_sequence: 1,
                    messages: Vec::new(),
                    checkpoint: None,
                    events: Vec::new(),
                }
            }
        };
        let secrets = secrets.into_iter().filter(|secret| secret.len() >= 4).collect();
        Ok(Self {
            session_id,
            file_path,
            state: Arc::new(tokio::sync::Mutex::new(state)),
            secrets: Arc::new(Mutex::new(secrets)),
            memory_error: Arc::new(Mutex::new(None)),
        })
    }

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
        append_record(&self.file_path, &record)?;
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

    pub async fn load_checkpoint(&self) -> Result<Option<Vec<Message>>> {
        Ok(self.state.lock().await.checkpoint.clone())
    }

    pub async fn save_checkpoint(&self, messages: Vec<Message>) -> Result<()> {
        if messages.is_empty() {
            return Err(session_error("run checkpoints cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        let mut combined = state.messages.clone();
        combined.extend(messages.iter().cloned());
        validate_checkpoint_history(&combined)?;
        let record = SessionRecord::RunCheckpoint {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            messages: messages.clone(),
        };
        append_record(&self.file_path, &record)?;
        state.next_sequence += 1;
        state.checkpoint = Some(messages);
        Ok(())
    }

    pub async fn promote_checkpoint(&self, messages: Vec<Message>) -> Result<()> {
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        let checkpoint = state
            .checkpoint
            .clone()
            .ok_or_else(|| session_error("run checkpoint is missing"))?;
        let mut promoted = checkpoint;
        promoted.extend(messages);
        let mut combined = state.messages.clone();
        combined.extend(promoted.iter().cloned());
        validate_canonical_history(&combined)?;
        let record = SessionRecord::CheckpointPromoted {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            messages: promoted.clone(),
        };
        append_record(&self.file_path, &record)?;
        state.next_sequence += 1;
        state.messages.extend(promoted);
        state.checkpoint = None;
        Ok(())
    }

    pub fn take_memory_error(&self) -> Option<String> {
        self.memory_error.lock().ok().and_then(|mut error| error.take())
    }

    pub fn add_secrets(&self, secrets: impl IntoIterator<Item = String>) -> Result<()> {
        let mut current = self
            .secrets
            .lock()
            .map_err(|_| session_error("session credential guard failed"))?;
        current.extend(secrets.into_iter().filter(|secret| secret.len() >= 4));
        current.sort();
        current.dedup();
        let persisted = std::fs::read_to_string(&self.file_path)?;
        if current.iter().any(|secret| persisted.contains(secret)) {
            return Err(session_error(
                "session contains credential material and cannot be resumed",
            ));
        }
        Ok(())
    }

    pub fn redact_credentials(&self, value: &str) -> String {
        let Ok(secrets) = self.secrets.lock() else {
            return "[REDACTED]".to_string();
        };
        secrets.iter().fold(value.to_string(), |redacted, secret| {
            redacted.replace(secret, "[REDACTED]")
        })
    }

    pub fn list_sessions(sessions_dir: &Path) -> Result<Vec<String>> {
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(sessions_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
            {
                ids.push(stem.to_string());
            }
        }
        ids.sort();
        ids.reverse();
        Ok(ids)
    }

    async fn append_messages(&self, conversation_id: &str, messages: Vec<Message>) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        if messages.is_empty() {
            return Err(session_error("canonical message batches cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        if state.checkpoint.is_some() {
            return Err(session_error(
                "pending run checkpoint must be continued before appending history",
            ));
        }
        let mut combined = state.messages.clone();
        combined.extend(messages.iter().cloned());
        validate_canonical_history(&combined)?;
        let record = SessionRecord::CanonicalMessages {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            messages: messages.clone(),
        };
        append_record(&self.file_path, &record)?;
        state.next_sequence += 1;
        state.messages.extend(messages);
        Ok(())
    }

    async fn clear_messages(&self, conversation_id: &str) -> Result<()> {
        self.ensure_conversation(conversation_id)?;
        let mut state = self.state.lock().await;
        let record = SessionRecord::CanonicalReset {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
        };
        append_record(&self.file_path, &record)?;
        state.next_sequence += 1;
        state.messages.clear();
        state.checkpoint = None;
        Ok(())
    }

    fn ensure_conversation(&self, conversation_id: &str) -> Result<()> {
        if conversation_id != self.session_id {
            return Err(session_error(format!(
                "conversation identity mismatch: expected {}, got {conversation_id}",
                self.session_id
            )));
        }
        Ok(())
    }

    fn reject_secrets<T: Serialize>(&self, value: &T) -> Result<()> {
        let encoded = serde_json::to_string(value).map_err(|_| session_error("session record serialization failed"))?;
        let secrets = self
            .secrets
            .lock()
            .map_err(|_| session_error("session credential guard failed"))?;
        if secrets.iter().any(|secret| encoded.contains(secret)) {
            return Err(session_error("session record contains credential material"));
        }
        Ok(())
    }

    fn remember_memory_error(&self, error: &AppError) {
        if let Ok(mut current) = self.memory_error.lock() {
            *current = Some(error.to_string());
        }
    }
}

impl ConversationMemory for SessionManager {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            if let Err(error) = self.ensure_conversation(conversation_id) {
                self.remember_memory_error(&error);
                return Err(MemoryError::backend(error));
            }
            Ok(self.state.lock().await.messages.clone())
        })
    }

    fn append<'a>(
        &'a self,
        conversation_id: &'a str,
        messages: Vec<Message>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            self.append_messages(conversation_id, messages).await.map_err(|error| {
                self.remember_memory_error(&error);
                MemoryError::backend(error)
            })
        })
    }

    fn clear<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            self.clear_messages(conversation_id).await.map_err(|error| {
                self.remember_memory_error(&error);
                MemoryError::backend(error)
            })
        })
    }
}

fn new_session_id() -> String {
    format!(
        "{}_{}",
        Utc::now().format("%Y%m%d_%H%M%S"),
        &uuid::Uuid::new_v4().to_string()[..8]
    )
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(session_error("invalid session id"));
    }
    Ok(())
}

fn create_session_file(path: &Path, session_id: &str) -> Result<()> {
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

fn append_record(path: &Path, record: &SessionRecord) -> Result<()> {
    let mut line = serde_json::to_vec(record).map_err(|_| session_error("session record serialization failed"))?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(&line)?;
    file.sync_data()?;
    Ok(())
}

fn load_file(path: &Path, expected_id: &str) -> Result<StoreState> {
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

fn parse_header(line: &[u8]) -> Result<SessionHeader> {
    let value: Value = serde_json::from_slice(line).map_err(|_| session_error("legacy session cannot be resumed"))?;
    if value.get("record_type").and_then(Value::as_str) != Some("header") {
        return Err(session_error("legacy session cannot be resumed"));
    }
    if value.get("version").is_none() {
        return Err(session_error("session is missing the mandatory version header"));
    }
    serde_json::from_value(value).map_err(|_| session_error("session version header is malformed"))
}

fn validate_header(header: &SessionHeader, expected_id: &str) -> Result<()> {
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

fn apply_record(state: &mut StoreState, record: SessionRecord, expected_id: &str) -> Result<()> {
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
        } => (*sequence, session_id),
    };
    if session_id != expected_id {
        return Err(session_error("session record identity mismatch"));
    }
    if sequence != state.next_sequence {
        return Err(session_error("session record ordering is invalid"));
    }
    match record {
        SessionRecord::CanonicalMessages { messages, .. } => {
            if messages.is_empty() {
                return Err(session_error("canonical message batches cannot be empty"));
            }
            state.messages.extend(messages);
            validate_canonical_history(&state.messages)?;
        }
        SessionRecord::CanonicalReset { .. } => {
            state.messages.clear();
            state.checkpoint = None;
        }
        SessionRecord::RunCheckpoint { messages, .. } => {
            if messages.is_empty() {
                return Err(session_error("run checkpoints cannot be empty"));
            }
            let mut combined = state.messages.clone();
            combined.extend(messages.iter().cloned());
            validate_checkpoint_history(&combined)?;
            state.checkpoint = Some(messages);
        }
        SessionRecord::CheckpointPromoted { messages, .. } => {
            let checkpoint = state
                .checkpoint
                .as_ref()
                .ok_or_else(|| session_error("checkpoint promotion ordering is invalid"))?;
            if messages.is_empty() || !messages.starts_with(checkpoint) {
                return Err(session_error("checkpoint promotion does not match pending history"));
            }
            state.messages.extend(messages);
            validate_canonical_history(&state.messages)?;
            state.checkpoint = None;
        }
        SessionRecord::AuditEvent { event, .. } => state.events.push(event),
    }
    state.next_sequence += 1;
    Ok(())
}

fn validate_canonical_history(messages: &[Message]) -> Result<()> {
    validate_history(messages, true)
}

fn validate_checkpoint_history(messages: &[Message]) -> Result<()> {
    validate_history(messages, false)
}

fn validate_history(messages: &[Message], require_assistant_end: bool) -> Result<()> {
    let mut seen_calls = HashSet::new();
    let mut pending = Vec::new();
    let mut can_accept_assistant = false;
    let mut last_was_assistant = false;
    for message in messages {
        match message {
            Message::System { .. } => {
                return Err(session_error("system messages are not canonical conversation memory"));
            }
            Message::User { content } => {
                if content.is_empty() {
                    return Err(session_error("canonical message role ordering is invalid"));
                }
                validate_user_content(content, &mut pending)?;
                can_accept_assistant = true;
                last_was_assistant = false;
            }
            Message::Assistant { content, .. } => {
                if !can_accept_assistant || content.is_empty() || !pending.is_empty() {
                    return Err(session_error("canonical message role ordering is invalid"));
                }
                for item in content {
                    if let AssistantContent::ToolCall(call) = item {
                        if !seen_calls.insert(call.id.to_string()) {
                            return Err(session_error("canonical tool-call id is duplicated"));
                        }
                        pending.push((call.id.to_string(), call.function.name.clone()));
                    }
                }
                can_accept_assistant = false;
                last_was_assistant = true;
            }
        }
    }
    if !pending.is_empty() {
        return Err(session_error("canonical history contains a dangling tool call"));
    }
    if require_assistant_end && !messages.is_empty() && !last_was_assistant {
        return Err(session_error(
            "canonical history does not end with an assistant message",
        ));
    }
    Ok(())
}

fn validate_user_content(content: &[UserContent], pending: &mut Vec<(String, String)>) -> Result<()> {
    let results = content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some((result.call.as_str(), result.name.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        if !results.is_empty() {
            return Err(session_error("canonical history contains an orphaned tool result"));
        }
        return Ok(());
    }
    if results.len() != content.len() || results.len() != pending.len() {
        return Err(session_error(
            "canonical tool calls do not have exactly one result each",
        ));
    }
    for ((result_id, result_name), (call_id, call_name)) in results.iter().zip(pending.iter()) {
        if *result_id != call_id || *result_name != call_name {
            return Err(session_error("canonical tool-call/result correlation is invalid"));
        }
    }
    pending.clear();
    Ok(())
}

fn session_error(message: impl Into<String>) -> AppError {
    AppError::Session(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::message::{ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent};

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("session_test_{}", uuid::Uuid::new_v4()))
    }

    fn complete_tool_turn(ids: &[&str]) -> Vec<Message> {
        let calls = ids
            .iter()
            .map(|id| {
                AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new(*id).unwrap(),
                    ToolFunction::new("read".to_string(), serde_json::json!({"path": id})),
                ))
            })
            .collect();
        let results = ids
            .iter()
            .map(|id| {
                UserContent::ToolResult(ToolResult {
                    call: ToolCallId::new(*id).unwrap(),
                    provider: None,
                    name: "read".to_string(),
                    content: vec![ToolResultContent::text("ok")],
                })
            })
            .collect();
        vec![
            Message::user("read files"),
            Message::Assistant {
                id: None,
                content: calls,
            },
            Message::User { content: results },
            Message::assistant("done"),
        ]
    }

    #[tokio::test]
    async fn empty_v2_session_round_trips_after_reopen() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        assert!(store.load_messages().await.unwrap().is_empty());
        drop(store);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(reopened.load_messages().await.unwrap().is_empty());
    }

    #[test]
    fn resume_requires_supported_versioned_header() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for (id, body, expected) in [
            ("legacy", r#"{"id":"old"}\n"#, "legacy session"),
            (
                "missing",
                r#"{"record_type":"header","session_id":"missing"}\n"#,
                "mandatory version",
            ),
            (
                "wrong",
                r#"{"record_type":"header","version":1,"session_id":"wrong","created_at":"2025-01-01T00:00:00Z"}\n"#,
                "unsupported session version",
            ),
            (
                "future",
                r#"{"record_type":"header","version":3,"session_id":"future","created_at":"2025-01-01T00:00:00Z"}\n"#,
                "unsupported session version",
            ),
        ] {
            std::fs::write(dir.join(format!("{id}.jsonl")), body.replace("\\n", "\n")).unwrap();
            let error = SessionManager::new(&dir, Some(id)).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn missing_and_unknown_sessions_fail_clearly() {
        let dir = temp_dir();
        let missing = SessionManager::new(&dir, Some("unknown")).unwrap_err().to_string();
        assert!(missing.contains("unknown session id"));
        std::fs::write(dir.join("empty.jsonl"), "").unwrap();
        let empty = SessionManager::new(&dir, Some("empty")).unwrap_err().to_string();
        assert!(empty.contains("mandatory version header"));
    }

    #[tokio::test]
    async fn canonical_memory_round_trips_multi_turn_and_multi_tool_order() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        ConversationMemory::append(&store, &id, complete_tool_turn(&["call-1", "call-2"]))
            .await
            .unwrap();
        ConversationMemory::append(&store, &id, vec![Message::user("next"), Message::assistant("answer")])
            .await
            .unwrap();
        let expected = ConversationMemory::load(&store, &id).await.unwrap();
        drop(store);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(ConversationMemory::load(&reopened, &id).await.unwrap(), expected);
    }

    #[tokio::test]
    async fn rejects_orphan_dangling_and_miscorrelated_tools() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        let mut dangling = complete_tool_turn(&["call-1"]);
        dangling.truncate(2);
        assert!(ConversationMemory::append(&store, &id, dangling).await.is_err());

        let orphan = vec![complete_tool_turn(&["call-1"])[2].clone(), Message::assistant("done")];
        assert!(ConversationMemory::append(&store, &id, orphan).await.is_err());

        let mut wrong = complete_tool_turn(&["call-1"]);
        if let Message::User { content } = &mut wrong[2]
            && let UserContent::ToolResult(result) = &mut content[0]
        {
            result.call = ToolCallId::new("other").unwrap();
        }
        assert!(ConversationMemory::append(&store, &id, wrong).await.is_err());
        assert!(store.load_messages().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_identity_failures_do_not_change_history() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        assert!(ConversationMemory::load(&store, "wrong-id").await.is_err());
        assert!(
            ConversationMemory::append(
                &store,
                "wrong-id",
                vec![Message::user("prompt"), Message::assistant("answer")],
            )
            .await
            .is_err()
        );
        assert!(store.load_messages().await.unwrap().is_empty());
        let error = store.take_memory_error().unwrap();
        assert!(error.contains("identity mismatch"));
    }

    #[tokio::test]
    async fn clear_preserves_file_and_audit_but_starts_fresh_history() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        ConversationMemory::append(&store, &id, vec![Message::user("old"), Message::assistant("answer")])
            .await
            .unwrap();
        store
            .append_event(
                SessionEventKind::AssistantResponse,
                serde_json::json!({"status":"complete"}),
            )
            .await
            .unwrap();
        ConversationMemory::clear(&store, &id).await.unwrap();
        assert!(ConversationMemory::load(&store, &id).await.unwrap().is_empty());
        assert_eq!(store.load_events().await.unwrap().len(), 1);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(reopened.load_messages().await.unwrap().is_empty());
        assert_eq!(reopened.load_events().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_appends_are_serialized_without_interleaving() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let tasks = (0..20).map(|index| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .append_event(SessionEventKind::UsageMetrics, serde_json::json!({"index":index}))
                    .await
                    .unwrap();
            })
        });
        futures::future::join_all(tasks).await;
        let reopened = SessionManager::new(&dir, Some(&store.session_id)).unwrap();
        assert_eq!(reopened.load_events().await.unwrap().len(), 20);
    }

    #[tokio::test]
    async fn budget_checkpoint_resumes_and_promotes_atomically_after_success() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        ConversationMemory::append(
            &store,
            &id,
            vec![Message::user("earlier"), Message::assistant("answer")],
        )
        .await
        .unwrap();
        let mut checkpoint = complete_tool_turn(&["call-1", "call-2"]);
        checkpoint.pop();
        store.save_checkpoint(checkpoint.clone()).await.unwrap();
        assert_eq!(store.load_messages().await.unwrap().len(), 2);
        assert_eq!(store.load_checkpoint().await.unwrap(), Some(checkpoint.clone()));
        assert!(
            ConversationMemory::append(&store, &id, vec![Message::user("must wait"), Message::assistant("no")],)
                .await
                .is_err()
        );

        drop(store);
        let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(resumed.load_checkpoint().await.unwrap(), Some(checkpoint.clone()));
        resumed
            .promote_checkpoint(vec![Message::user("please continue"), Message::assistant("done")])
            .await
            .unwrap();
        assert!(resumed.load_checkpoint().await.unwrap().is_none());
        assert_eq!(resumed.load_messages().await.unwrap().len(), 7);

        drop(resumed);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(reopened.load_checkpoint().await.unwrap().is_none());
        assert_eq!(reopened.load_messages().await.unwrap().len(), 7);
    }

    #[tokio::test]
    async fn budget_checkpoint_rejects_dangling_tools_and_credentials() {
        let dir = temp_dir();
        let store = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
        let mut dangling = complete_tool_turn(&["call-1"]);
        dangling.truncate(2);
        assert!(store.save_checkpoint(dangling).await.is_err());
        let error = store
            .save_checkpoint(vec![Message::user("credential-sentinel")])
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("credential-sentinel"));
        assert!(store.load_checkpoint().await.unwrap().is_none());
        assert!(
            !std::fs::read_to_string(&store.file_path)
                .unwrap()
                .contains("credential-sentinel")
        );
    }

    #[tokio::test]
    async fn cancellation_fixtures_remain_parseable_and_resumable() {
        for boundary in [
            "before_first_token",
            "during_text",
            "between_call_result",
            "during_tool",
        ] {
            let dir = temp_dir();
            let store = SessionManager::new(&dir, None).unwrap();
            let id = store.session_id.clone();
            store
                .append_event(
                    SessionEventKind::Cancellation,
                    serde_json::json!({"boundary": boundary, "terminal": true}),
                )
                .await
                .unwrap();
            drop(store);

            let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
            assert!(reopened.load_messages().await.unwrap().is_empty());
            ConversationMemory::append(
                &reopened,
                &id,
                vec![Message::user("after cancel"), Message::assistant("resumed")],
            )
            .await
            .unwrap();
            drop(reopened);
            let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
            assert_eq!(resumed.load_messages().await.unwrap().len(), 2);
        }
    }

    #[test]
    fn malformed_committed_records_fail_but_incomplete_tail_is_ignored() {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&store.file_path)
            .unwrap()
            .write_all(b"{interrupted")
            .unwrap();
        assert!(SessionManager::new(&dir, Some(&id)).is_ok());

        let bad_dir = temp_dir();
        let bad = SessionManager::new(&bad_dir, None).unwrap();
        let bad_id = bad.session_id.clone();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&bad.file_path)
            .unwrap()
            .write_all(b"{malformed}\n")
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&bad.file_path)
            .unwrap()
            .write_all(b"{\"record_type\":\"canonical_reset\",\"sequence\":1,\"session_id\":\"ignored\",\"timestamp\":\"2025-01-01T00:00:00Z\"}\n")
            .unwrap();
        let error = SessionManager::new(&bad_dir, Some(&bad_id)).unwrap_err().to_string();
        assert!(error.contains("malformed committed record"));
    }

    #[tokio::test]
    async fn credential_values_are_rejected_without_persistence_or_error_echo() {
        let dir = temp_dir();
        let store = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
        assert_eq!(
            store.redact_credentials("prefix credential-sentinel suffix"),
            "prefix [REDACTED] suffix"
        );
        let error = store
            .append_event(
                SessionEventKind::UserMessage,
                serde_json::json!({"text":"credential-sentinel"}),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("credential-sentinel"));
        let persisted = std::fs::read_to_string(&store.file_path).unwrap();
        assert!(!persisted.contains("credential-sentinel"));
    }
}
