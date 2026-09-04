mod branch;
mod checkpoint;
pub mod context;
mod cwd;
mod event;
pub mod export;
pub mod format;
mod fs;
mod memory;
mod secrets;
pub mod summary;
#[cfg(test)]
mod tests;
pub mod tree;
mod turns;
mod validation;

use secrets::SecretGuard;

pub use cwd::{last_session_for_cwd, record_session_for_cwd};
pub use format::{SessionEvent, SessionEventKind, SessionHeader, SessionRecord, StoreState};
pub(crate) use format::{create_session_file, load_file};
pub(crate) use fs::{
    new_session_id, session_error, set_private_directory_permissions, set_private_file_permissions, validate_session_id,
};
pub use summary::{SessionSummary, delete_session, list_session_summaries, list_sessions};
pub use tree::{SessionTree, TreeNodeData, TreeNodeKind};
pub use turns::ConversationTurn;
use validation::CanonicalHistory;

use crate::error::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SessionManager {
    pub session_id: String,
    pub file_path: PathBuf,
    pub(crate) state: Arc<tokio::sync::Mutex<StoreState>>,
    pub(crate) secrets: Arc<SecretGuard>,
    pub(crate) memory_error: Arc<Mutex<Option<String>>>,
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

    pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
        cwd::record_session_for_cwd(sessions_dir, cwd, session_id)
    }

    pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
        cwd::last_session_for_cwd(sessions_dir, cwd)
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
        summary::list_sessions(sessions_dir)
    }

    pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
        summary::list_session_summaries(sessions_dir)
    }

    pub fn delete_session(sessions_dir: &Path, session_id: &str) -> Result<()> {
        summary::delete_session(sessions_dir, session_id)
    }

    pub(crate) fn reject_secrets<T: Serialize>(&self, value: &T) -> Result<()> {
        self.secrets.reject_in(value)
    }
}
