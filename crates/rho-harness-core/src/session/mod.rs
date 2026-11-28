//! Durable session storage: append-only JSONL records, history tree with
//! branching and checkpoints, compaction bookkeeping, secret redaction, and
//! export. This module is the single source of truth for conversation state.

mod append;
mod branch;
mod checkpoint;
pub mod compaction;
mod cwd;
mod event;
pub mod export;
mod format;
mod fs;
mod lock;
mod memory;
pub mod prune;
mod secrets;
mod subagents;
pub mod summary;
mod summary_scan;
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
pub use crate::secrets::redact_credentials_text;
pub use checkpoint::unfinished_turn_closing;
pub(crate) use format::create_session_file;
pub use format::{SessionEvent, SessionEventKind, SessionHeader, SessionRecord, StoreState};
pub(crate) use fs::{
    new_session_id, run_blocking, session_error, set_private_directory_permissions, set_private_file_permissions,
    validate_session_id,
};
pub use prune::{prune_empty_sessions_async, prune_expired_sessions_async};
pub use subagents::{SubagentContext, SubagentContextSummary, SubagentContexts};
pub use summary::{SessionSummary, delete_session_async, list_session_summaries_async};
pub use tree::{SessionTree, TitleSource, TreeNodeData, TreeNodeKind};
pub use turns::{ConversationTurn, RewindOutcome};
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
    pub(crate) title: Arc<lock::TitleCell>,
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
        tree: SessionTree::new(),
        subagents: SubagentContexts::default(),
        integrity: CanonicalHistory::new(),
    }
}

/// Takes the session lock first, so a session held elsewhere is refused before its file is parsed.
fn open_session(path: &Path, session_id: &str, resume: bool) -> Result<Arc<lock::SessionLock>> {
    if resume {
        set_private_file_permissions(path)?;
        lock::acquire(path, |locked| format::load_and_upgrade(path, session_id, locked)).map_err(|error| match error {
            crate::error::AppError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                session_error(format!("unknown session id: {session_id}"))
            }
            other => other,
        })
    } else {
        create_session_file(path, session_id)?;
        lock::acquire(path, |_| Ok(empty_store_state()))
    }
}

fn record_cwd_if_present(sessions_dir: &Path, session_id: &str) {
    if let Ok(cwd) = std::env::current_dir()
        && let Err(error) = SessionManager::record_session_for_cwd(sessions_dir, &cwd, session_id)
    {
        tracing::warn!(session_id, cwd = %cwd.display(), %error, "could not index session by working directory");
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
        let session_lock = open_session(&file_path, &session_id, resume_id.is_some())?;
        record_cwd_if_present(sessions_dir, &session_id);
        Ok(Self::assemble(session_id, file_path, secrets, session_lock))
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
        secrets: Vec<String>,
        session_lock: Arc<lock::SessionLock>,
    ) -> Self {
        Self {
            session_id,
            file_path,
            state: Arc::clone(session_lock.state()),
            title: Arc::clone(session_lock.title()),
            secrets: Arc::new(SecretGuard::new(secrets)),
            _session_lock: session_lock,
        }
    }

    pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
        cwd::record_session_for_cwd(sessions_dir, cwd, session_id)
    }

    pub async fn record_session_for_cwd_async(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
        let (sessions_dir, cwd, session_id) = (sessions_dir.to_path_buf(), cwd.to_path_buf(), session_id.to_string());
        run_blocking(move || cwd::record_session_for_cwd(&sessions_dir, &cwd, &session_id)).await
    }

    pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd(sessions_dir, cwd)
    }

    pub async fn last_session_for_cwd_async(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd_async(sessions_dir, cwd).await
    }

    pub fn cwd_for_session(sessions_dir: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        cwd::cwd_for_session(sessions_dir, session_id)
    }

    pub async fn cwd_for_session_async(sessions_dir: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        cwd::cwd_for_session_async(sessions_dir, session_id).await
    }

    pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
        summary::list_session_summaries(sessions_dir)
    }

    pub async fn add_secrets_async(&self, secrets: impl IntoIterator<Item = String>) -> Result<()> {
        let needles = self.secrets.add(secrets)?;
        if needles.is_empty() {
            return Ok(());
        }
        let path = self.file_path.clone();
        let found = run_blocking(move || secrets::file_contains_any(&path, &needles)).await?;
        self.secrets.record_scan(found)
    }

    pub fn redact_credentials(&self, value: &str) -> String {
        self.secrets.redact(value)
    }

    /// Redacts a streamed chunk, keeping any tail that could still complete a secret in `carry`.
    /// Once the stream ends, `carry` holds no complete secret and can be shown as is.
    pub fn redact_credentials_streamed(&self, carry: &mut String, chunk: &str) -> String {
        self.secrets.redact_stream(carry, chunk)
    }

    /// Prunes expired sessions in the background, at most once per sessions directory per process.
    pub fn spawn_auto_prune(&self, retention_days: u32) -> Option<tokio::task::JoinHandle<()>> {
        static PRUNED_DIRS: std::sync::Mutex<std::collections::BTreeSet<PathBuf>> =
            std::sync::Mutex::new(std::collections::BTreeSet::new());
        let active_id = self.session_id.clone();
        let sessions_dir = self.file_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        if !crate::locking::lock(&PRUNED_DIRS).insert(sessions_dir.clone()) {
            return None;
        }
        Some(tokio::spawn(async move {
            if let Err(error) = prune_expired_sessions_async(&sessions_dir, &active_id, retention_days).await {
                tracing::warn!(dir = %sessions_dir.display(), %error, "could not prune expired sessions");
            }
        }))
    }

    pub(crate) fn reject_secrets<T: Serialize>(&self, value: &T) -> Result<()> {
        self.secrets.reject_in(value)
    }
}
