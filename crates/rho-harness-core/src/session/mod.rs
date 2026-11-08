//! Durable session storage: append-only JSONL records, history tree with
//! branching and checkpoints, compaction bookkeeping, secret redaction, and
//! export. This module is the single source of truth for conversation state.

mod branch;
mod checkpoint;
pub mod compaction;
mod cwd;
mod event;
pub mod export;
pub mod format;
mod fs;
mod lock;
mod memory;
pub mod prune;
mod secrets;
pub mod summary;
#[cfg(test)]
mod tests;
pub mod tree;
pub mod turns;
mod validation;

use secrets::SecretGuard;

pub use compaction::{
    CompactionCut, CompactionDetails, CompactionMetadata, MAX_TOOL_RESULT_CHARS, SUMMARIZATION_PROMPT,
    SUMMARIZATION_SYSTEM_PROMPT, TURN_PREFIX_SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT,
    build_summarization_prompt, build_turn_prefix_prompt, build_update_summarization_prompt,
    compaction_summary_message, compose_compaction_summary, extract_file_ops, generate_fallback_summary,
    merge_split_turn_summary, render_file_lists_xml, serialize_conversation,
};

pub const DEFAULT_CONTEXT_WINDOW_MESSAGES: usize = 24;
pub const DEFAULT_COMPACTION_MAX_BYTES: usize = 8 * 1024;
pub use format::{SessionEvent, SessionEventKind, SessionHeader, SessionRecord, StoreState};
pub(crate) use format::{create_session_file, load_file};
pub(crate) use fs::{
    new_session_id, run_blocking, session_error, set_private_directory_permissions, set_private_file_permissions,
    validate_session_id,
};
pub use prune::{prune_empty_sessions_async, prune_expired_sessions_async};
pub use summary::{SessionSummary, delete_session_async, list_session_summaries, list_session_summaries_async};
pub use tree::{SessionTree, TreeNodeData, TreeNodeKind};
pub use turns::ConversationTurn;
use validation::CanonicalHistory;

use crate::error::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct SessionManager {
    pub session_id: String,
    pub file_path: PathBuf,
    pub(crate) state: Arc<tokio::sync::Mutex<StoreState>>,
    pub(crate) secrets: Arc<SecretGuard>,
    pub(crate) _session_lock: Arc<lock::SessionLock>,
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

fn prepare_session_id(resume_id: Option<&str>) -> Result<String> {
    let session_id = resume_id.map_or_else(new_session_id, str::to_string);
    validate_session_id(&session_id)?;
    Ok(session_id)
}

fn empty_store_state() -> StoreState {
    StoreState {
        created_at: chrono::Utc::now(),
        next_sequence: 1,
        messages: Vec::new(),
        checkpoint: None,
        events: Vec::new(),
        tree: SessionTree::new(),
        integrity: CanonicalHistory::new(),
    }
}

fn load_or_create_state(path: &Path, session_id: &str, resume: bool) -> Result<StoreState> {
    if resume {
        set_private_file_permissions(path)?;
        load_file(path, session_id)
    } else {
        create_session_file(path, session_id)?;
        Ok(empty_store_state())
    }
}

fn record_cwd_if_present(sessions_dir: &Path, session_id: &str) {
    if let Ok(cwd) = std::env::current_dir() {
        let _ = SessionManager::record_session_for_cwd(sessions_dir, &cwd, session_id);
    }
}

fn session_paths(sessions_dir: &Path, resume_id: Option<&str>) -> Result<(String, PathBuf)> {
    let session_id = prepare_session_id(resume_id)?;
    let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
    Ok((session_id, file_path))
}

impl SessionManager {
    pub fn new(sessions_dir: &Path, resume_id: Option<&str>) -> Result<Self> {
        Self::new_with_secrets(sessions_dir, resume_id, Vec::new())
    }

    pub async fn new_async(sessions_dir: &Path, resume_id: Option<&str>) -> Result<Self> {
        Self::new_with_secrets_async(sessions_dir, resume_id, Vec::new()).await
    }

    pub fn new_with_secrets(sessions_dir: &Path, resume_id: Option<&str>, secrets: Vec<String>) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)?;
        set_private_directory_permissions(sessions_dir)?;
        let (session_id, file_path) = session_paths(sessions_dir, resume_id)?;
        let state = load_or_create_state(&file_path, &session_id, resume_id.is_some())?;
        let session_lock = lock::acquire(&file_path)?;
        record_cwd_if_present(sessions_dir, &session_id);
        Ok(Self::assemble(session_id, file_path, state, secrets, session_lock))
    }

    pub async fn new_with_secrets_async(
        sessions_dir: &Path,
        resume_id: Option<&str>,
        secrets: Vec<String>,
    ) -> Result<Self> {
        let sessions_dir = sessions_dir.to_path_buf();
        let resume_id = resume_id.map(str::to_owned);
        run_blocking(move || Self::new_with_secrets(&sessions_dir, resume_id.as_deref(), secrets)).await
    }

    fn assemble(
        session_id: String,
        file_path: PathBuf,
        state: StoreState,
        secrets: Vec<String>,
        session_lock: Arc<lock::SessionLock>,
    ) -> Self {
        Self {
            session_id,
            file_path,
            state: Arc::new(tokio::sync::Mutex::new(state)),
            secrets: Arc::new(SecretGuard::new(secrets)),
            _session_lock: session_lock,
        }
    }

    pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
        cwd::record_session_for_cwd(sessions_dir, cwd, session_id)
    }

    pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd(sessions_dir, cwd)
    }

    pub async fn last_session_for_cwd_async(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd_async(sessions_dir, cwd).await
    }

    pub async fn add_secrets_async(&self, secrets: impl IntoIterator<Item = String>) -> Result<()> {
        let persisted = tokio::fs::read_to_string(&self.file_path).await?;
        self.secrets.add(secrets, &persisted)
    }

    pub fn redact_credentials(&self, value: &str) -> String {
        self.secrets.redact(value)
    }

    pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
        summary::list_session_summaries(sessions_dir)
    }

    pub async fn list_session_summaries_async(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
        summary::list_session_summaries_async(sessions_dir).await
    }

    pub fn spawn_auto_prune(&self, retention_days: u32) {
        let active_id = self.session_id.clone();
        let sessions_dir = self.file_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        tokio::spawn(async move {
            let _ = prune_expired_sessions_async(&sessions_dir, &active_id, retention_days).await;
        });
    }

    pub async fn delete_session_async(sessions_dir: &Path, session_id: &str) -> Result<()> {
        summary::delete_session_async(sessions_dir, session_id).await
    }

    pub(crate) fn reject_secrets<T: Serialize>(&self, value: &T) -> Result<()> {
        self.secrets.reject_in(value)
    }
}
