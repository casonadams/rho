pub mod context;
mod format;
mod secrets;
#[cfg(test)]
mod tests;
mod validation;

use secrets::SecretGuard;

pub use format::{SessionEvent, SessionEventKind, SessionHeader, SessionRecord, StoreState};
pub(crate) use format::{append_record, create_session_file, load_file};
pub(crate) use validation::{validate_canonical_history, validate_checkpoint_history};

use crate::error::{AppError, Result};
use chrono::Utc;
use rig::memory::{ConversationMemory, MemoryError};
use rig::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub user_prompt: String,
    pub assistant_preview: String,
    pub tool_calls_count: usize,
}

#[derive(Clone)]
pub struct SessionManager {
    pub session_id: String,
    pub file_path: PathBuf,
    state: Arc<tokio::sync::Mutex<StoreState>>,
    secrets: Arc<SecretGuard>,
    memory_error: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for SessionManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionManager")
            .field("session_id", &self.session_id)
            .field("file_path", &self.file_path)
            .finish_non_exhaustive()
    }
}

impl SessionManager {
    pub fn new(sessions_dir: &Path, resume_id: Option<&str>) -> Result<Self> {
        Self::new_with_secrets(sessions_dir, resume_id, Vec::new())
    }

    pub fn new_with_secrets(sessions_dir: &Path, resume_id: Option<&str>, secrets: Vec<String>) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)?;
        set_private_directory_permissions(sessions_dir)?;
        let session_id = resume_id.map_or_else(new_session_id, str::to_string);
        validate_session_id(&session_id)?;
        let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
        let state = match resume_id {
            Some(_) => {
                set_private_file_permissions(&file_path)?;
                load_file(&file_path, &session_id)?
            }
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
        let secrets = Arc::new(SecretGuard::new(secrets));
        if let Ok(cwd) = std::env::current_dir() {
            let _ = Self::record_session_for_cwd(sessions_dir, &cwd, &session_id);
        }
        Ok(Self {
            session_id,
            file_path,
            state: Arc::new(tokio::sync::Mutex::new(state)),
            secrets,
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
        let persisted = std::fs::read_to_string(&self.file_path)?;
        self.secrets.add(secrets, &persisted)
    }

    pub fn redact_credentials(&self, value: &str) -> String {
        self.secrets.redact(value)
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

    pub async fn load_turns(&self) -> Result<Vec<ConversationTurn>> {
        let messages = self.load_messages().await?;
        let mut turns = Vec::new();
        let mut current_prompt = String::new();
        let mut current_assistant = String::new();
        let mut current_tool_calls = 0;
        let mut turn_num = 0;

        for msg in messages {
            match msg {
                Message::User { content } => {
                    let has_text = content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_)));
                    if has_text && (!current_prompt.is_empty() || !current_assistant.is_empty()) {
                        turn_num += 1;
                        turns.push(ConversationTurn {
                            turn_number: turn_num,
                            user_prompt: std::mem::take(&mut current_prompt),
                            assistant_preview: std::mem::take(&mut current_assistant),
                            tool_calls_count: current_tool_calls,
                        });
                        current_tool_calls = 0;
                    }
                    for part in content {
                        match part {
                            rig::message::UserContent::Text(t) => {
                                if !current_prompt.is_empty() {
                                    current_prompt.push(' ');
                                }
                                current_prompt.push_str(&t.text);
                            }
                            rig::message::UserContent::ToolResult(_) => {
                                current_tool_calls += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Message::Assistant { content, .. } => {
                    for part in content {
                        match part {
                            rig::message::AssistantContent::Text(t) => {
                                if !current_assistant.is_empty() {
                                    current_assistant.push(' ');
                                }
                                current_assistant.push_str(&t.text);
                            }
                            rig::message::AssistantContent::ToolCall(_) => {
                                current_tool_calls += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Message::System { .. } => {}
            }
        }

        if !current_prompt.is_empty() || !current_assistant.is_empty() {
            turn_num += 1;
            turns.push(ConversationTurn {
                turn_number: turn_num,
                user_prompt: current_prompt,
                assistant_preview: current_assistant,
                tool_calls_count: current_tool_calls,
            });
        }

        Ok(turns)
    }

    pub async fn rewind_to_turn(&self, target_turn: usize) -> Result<usize> {
        let messages = self.load_messages().await?;
        let mut user_turn_count = 0;
        let mut cutoff_idx = 0;

        for (i, msg) in messages.iter().enumerate() {
            if matches!(msg, Message::User { content } if content.iter().any(|c| matches!(c, rig::message::UserContent::Text(_))))
            {
                user_turn_count += 1;
                if user_turn_count > target_turn {
                    break;
                }
            }
            cutoff_idx = i + 1;
        }

        if cutoff_idx == 0 || cutoff_idx >= messages.len() {
            return Ok(messages.len());
        }

        let retained = messages[..cutoff_idx].to_vec();
        self.clear_messages(&self.session_id).await?;
        if !retained.is_empty() {
            self.append_messages(&self.session_id, retained.clone()).await?;
        }
        Ok(retained.len())
    }

    pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
        std::fs::create_dir_all(sessions_dir)?;
        let index_file = sessions_dir.join(".last_sessions.json");
        let mut map: std::collections::BTreeMap<String, String> = if index_file.exists() {
            let content = std::fs::read_to_string(&index_file)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            std::collections::BTreeMap::new()
        };
        let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        map.insert(canonical_cwd.display().to_string(), session_id.to_string());
        let json = serde_json::to_string_pretty(&map).map_err(|e| AppError::Session(e.to_string()))?;
        std::fs::write(&index_file, json)?;
        Ok(())
    }

    pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        let index_file = sessions_dir.join(".last_sessions.json");
        if !index_file.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&index_file)?;
        let map: std::collections::BTreeMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
        let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        Ok(map.get(&canonical_cwd.display().to_string()).cloned())
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
        self.secrets.reject_in(value)
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

fn set_private_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if path.exists() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
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

fn session_error(message: impl Into<String>) -> AppError {
    AppError::Session(message.into())
}
