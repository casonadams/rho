pub mod context;
mod cwd;
mod format;
mod secrets;
#[cfg(test)]
mod tests;
pub mod tree;
mod turns;
mod validation;

use secrets::SecretGuard;

pub use cwd::{last_session_for_cwd, record_session_for_cwd};
pub use format::{SessionEvent, SessionEventKind, SessionHeader, SessionRecord, StoreState};
pub(crate) use format::{append_durable_record, append_record, create_session_file, load_file};
pub use tree::{SessionTree, TreeNodeData, TreeNodeKind};
pub use turns::ConversationTurn;
use validation::CanonicalHistory;

use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use rig::memory::{ConversationMemory, MemoryError};
use rig::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
    pub turn_count: usize,
    pub preview: String,
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
                    tree: SessionTree::new(),
                    integrity: CanonicalHistory::new(),
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

    pub async fn load_tree(&self) -> Result<SessionTree> {
        Ok(self.state.lock().await.tree.clone())
    }

    pub async fn active_leaf_id(&self) -> Result<Option<String>> {
        Ok(self.state.lock().await.tree.active_leaf_id.clone())
    }

    pub async fn switch_branch(&self, leaf_id: Option<String>) -> Result<Vec<Message>> {
        let mut state = self.state.lock().await;
        state.tree.set_active_leaf(leaf_id.clone());
        let messages = state.tree.active_messages();
        state.messages = messages.clone();
        let record = SessionRecord::ActiveLeafChanged {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            active_leaf_id: leaf_id,
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(messages)
    }

    pub async fn set_node_label(&self, node_id: &str, label: Option<String>) -> Result<()> {
        let mut state = self.state.lock().await;
        state.tree.set_node_label(node_id, label.clone());
        let record = SessionRecord::SessionLabel {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            node_id: node_id.to_string(),
            label,
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(())
    }

    pub async fn set_session_name(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        state.tree.set_session_name(name.to_string());
        let record = SessionRecord::SessionNamed {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            name: name.to_string(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        Ok(())
    }

    pub async fn get_session_name(&self) -> Result<Option<String>> {
        Ok(self.state.lock().await.tree.session_name.clone())
    }

    pub fn cached_session_name(&self) -> Option<String> {
        self.state.try_lock().ok().and_then(|s| s.tree.session_name.clone())
    }

    pub async fn append_branch_summary(&self, summary: &str, source_leaf_id: &str) -> Result<()> {
        self.reject_secrets(&summary)?;
        let mut state = self.state.lock().await;
        let parent_id = state.tree.active_leaf_id.clone();
        let node_id = uuid::Uuid::new_v4().to_string();
        let summary_message = Message::assistant(format!("[Branch Summary from {source_leaf_id}]: {summary}"));
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: Utc::now(),
            kind: TreeNodeKind::BranchSummary,
            messages: vec![summary_message],
            label: Some("Branch Summary".to_string()),
            metadata: Some(serde_json::json!({ "source_leaf_id": source_leaf_id })),
        };
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
        Ok(())
    }

    pub async fn fork_session(
        &self,
        sessions_dir: &Path,
        target_leaf_or_turn_id: Option<&str>,
    ) -> Result<SessionManager> {
        let tree = self.load_tree().await?;
        let target_node_id = if let Some(id_or_turn) = target_leaf_or_turn_id {
            if let Ok(turn_num) = id_or_turn.parse::<usize>() {
                let turns = self.load_turns().await?;
                if turn_num > 0 && turn_num <= turns.len() {
                    let nodes = match &tree.active_leaf_id {
                        Some(leaf) => tree.ancestor_nodes(leaf),
                        None => Vec::new(),
                    };
                    nodes.get(turn_num.saturating_sub(1)).map(|n| n.id.clone())
                } else {
                    Some(id_or_turn.to_string())
                }
            } else {
                Some(id_or_turn.to_string())
            }
        } else {
            tree.active_leaf_id.clone()
        };

        let forked = SessionManager::new(sessions_dir, None)?;
        if let Some(target_id) = target_node_id {
            let ancestors = tree.ancestor_nodes(&target_id);
            for node in ancestors {
                forked
                    .append_messages(&forked.session_id, node.messages.clone())
                    .await?;
            }
        }
        Ok(forked)
    }

    pub async fn clone_session(&self, sessions_dir: &Path) -> Result<SessionManager> {
        self.fork_session(sessions_dir, None).await
    }

    pub async fn load_turns(&self) -> Result<Vec<ConversationTurn>> {
        let messages = self.load_messages().await?;
        Ok(turns::extract_turns(&messages))
    }

    pub async fn rewind_to_turn(&self, target_turn: usize) -> Result<usize> {
        let messages = self.load_messages().await?;
        let cutoff_idx = turns::calculate_rewind_cutoff(&messages, target_turn);

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
        cwd::record_session_for_cwd(sessions_dir, cwd, session_id)
    }

    pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd(sessions_dir, cwd)
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

    pub async fn load_checkpoint(&self) -> Result<Option<Vec<Message>>> {
        Ok(self.state.lock().await.checkpoint.clone())
    }

    pub async fn save_checkpoint(&self, messages: Vec<Message>) -> Result<()> {
        if messages.is_empty() {
            return Err(session_error("run checkpoints cannot be empty"));
        }
        self.reject_secrets(&messages)?;
        let mut state = self.state.lock().await;
        state.integrity.check_checkpoint_batch(&messages)?;
        let record = SessionRecord::RunCheckpoint {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            messages: messages.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
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
        state.integrity.check_canonical_batch(&promoted)?;
        let now = Utc::now();
        let node_id = uuid::Uuid::new_v4().to_string();
        let parent_id = state.tree.active_leaf_id.clone();
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: now,
            kind: TreeNodeKind::AssistantTurn,
            messages: promoted.clone(),
            label: None,
            metadata: None,
        };
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
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

    pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }
        let mut summaries = Vec::new();
        for entry in std::fs::read_dir(sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
                && let Ok(state) = load_file(&path, stem)
            {
                let metadata = std::fs::metadata(&path)?;
                let last_modified: DateTime<Utc> = metadata
                    .modified()
                    .map(DateTime::<Utc>::from)
                    .unwrap_or_else(|_| Utc::now());
                let turn_count = state.tree.len();
                let preview = state
                    .tree
                    .root_nodes()
                    .first()
                    .and_then(|n| {
                        n.messages.iter().find_map(|m| match m {
                            Message::User { content } => content.first().map(|c| match c {
                                rig::message::UserContent::Text(t) => t.text.clone(),
                                _ => String::new(),
                            }),
                            _ => None,
                        })
                    })
                    .unwrap_or_else(|| "Empty session".to_string());
                let preview_truncated = if preview.chars().count() > 50 {
                    format!("{}...", preview.chars().take(47).collect::<String>())
                } else {
                    preview
                };
                summaries.push(SessionSummary {
                    session_id: stem.to_string(),
                    name: state.tree.session_name,
                    created_at: last_modified,
                    last_modified,
                    turn_count,
                    preview: preview_truncated,
                });
            }
        }
        summaries.sort_by_key(|b| std::cmp::Reverse(b.last_modified));
        Ok(summaries)
    }

    pub fn delete_session(sessions_dir: &Path, session_id: &str) -> Result<()> {
        let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
        if file_path.exists() {
            std::fs::remove_file(file_path)?;
        }
        Ok(())
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
        state.integrity.check_canonical_batch(&messages)?;
        let now = Utc::now();
        let node_id = uuid::Uuid::new_v4().to_string();
        let parent_id = state.tree.active_leaf_id.clone();
        let node = TreeNodeData {
            id: node_id,
            parent_id,
            timestamp: now,
            kind: TreeNodeKind::UserTurn,
            messages: messages.clone(),
            label: None,
            metadata: None,
        };
        let record = SessionRecord::TreeNode {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            node: node.clone(),
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.tree.add_node(node);
        state.messages = state.tree.active_messages();
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
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.messages.clear();
        state.checkpoint = None;
        state.integrity.clear();
        state.tree = SessionTree::new();
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
