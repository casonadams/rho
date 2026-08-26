//! [`IntentHandle`](super::IntentHandle) lifecycle and persistence.
//!
//! Responsibilities split out of [`super`] (the `store` module):
//!
//! - Construction from a fresh spec (`create`) or an existing on-disk file
//!   (`open`).
//! - The optimistic-concurrency `update` helper that all mutating methods
//!   delegate to.
//! - File-level concerns: `create_file`, `persist`, and the secret-redaction
//!   guard `reject_secrets`.
//!
//! Index management lives in [`super::index`]; the
//! [`IntentSummary`](super::IntentSummary) projection used to feed the index
//! also lives there.

use super::{
    INTENT_VERSION, IntentHandle, IntentHeader, IntentSnapshot, IntentState, NewIntent, load_state, validate_intent_id,
};
use crate::error::{AppError, Result};
use chrono::Utc;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub(super) fn create(intents_dir: &Path, intent: NewIntent) -> Result<IntentHandle> {
    std::fs::create_dir_all(intents_dir)?;
    let state = IntentState::new(intent.spec, intent.workspace, intent.session_id);
    let file_path = intents_dir.join(format!("{}.jsonl", state.spec.id));
    let handle = IntentHandle {
        state: Arc::new(Mutex::new(state)),
        file_path,
        intents_dir: intents_dir.to_path_buf(),
        secrets: Arc::new(intent.secrets.into_iter().filter(|secret| secret.len() >= 4).collect()),
    };
    handle.create_file()?;
    handle.persist()?;
    Ok(handle)
}

pub(super) fn open(intents_dir: &Path, intent_id: &str, secrets: Vec<String>) -> Result<IntentHandle> {
    validate_intent_id(intent_id)?;
    let file_path = intents_dir.join(format!("{intent_id}.jsonl"));
    let state = load_state(&file_path, intent_id)?;
    Ok(IntentHandle {
        state: Arc::new(Mutex::new(state)),
        file_path,
        intents_dir: intents_dir.to_path_buf(),
        secrets: Arc::new(secrets.into_iter().filter(|secret| secret.len() >= 4).collect()),
    })
}

pub(super) fn update(handle: &IntentHandle, change: impl FnOnce(&mut IntentState) -> Result<()>) -> Result<()> {
    let previous = handle.snapshot()?;
    let mut candidate = previous.clone();
    change(&mut candidate)?;
    if candidate == previous {
        return Ok(());
    }
    candidate.revision += 1;
    *handle
        .state
        .lock()
        .map_err(|_| AppError::Intent("Active intent state is unavailable".to_string()))? = candidate;
    if let Err(error) = handle.persist() {
        if let Ok(mut state) = handle.state.lock() {
            *state = previous;
        }
        return Err(error);
    }
    Ok(())
}

impl IntentHandle {
    pub(super) fn create_file(&self) -> Result<()> {
        let state = self.snapshot()?;
        let header = IntentHeader {
            record_type: "intent_header".to_string(),
            version: INTENT_VERSION,
            intent_id: state.spec.id,
            created_at: Utc::now(),
        };
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.file_path)?;
        serde_json::to_writer(&mut file, &header)
            .map_err(|_| AppError::Intent("Intent header serialization failed".to_string()))?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        Ok(())
    }

    pub(super) fn persist(&self) -> Result<()> {
        let state = self.snapshot()?;
        self.reject_secrets(&state)?;
        let snapshot = IntentSnapshot {
            record_type: "intent_snapshot".to_string(),
            sequence: state.revision,
            timestamp: Utc::now(),
            state: state.clone(),
        };
        let mut line = serde_json::to_vec(&snapshot)
            .map_err(|_| AppError::Intent("Intent snapshot serialization failed".to_string()))?;
        line.push(b'\n');
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.file_path)?;
        file.write_all(&line)?;
        file.sync_data()?;
        super::index::update_index(&self.intents_dir, super::index::summary(&state, snapshot.timestamp))
    }

    pub(super) fn reject_secrets(&self, state: &IntentState) -> Result<()> {
        let encoded = serde_json::to_string(state)
            .map_err(|_| AppError::Intent("Intent snapshot serialization failed".to_string()))?;
        if self.secrets.iter().any(|secret| encoded.contains(secret)) {
            return Err(AppError::Intent("Intent contains credential material".to_string()));
        }
        Ok(())
    }
}
