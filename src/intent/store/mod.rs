mod handle;
mod index;

use super::IntentSpec;
use super::lifecycle::{IntentProgress, IntentState, IntentStatus};
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const INTENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IntentHeader {
    record_type: String,
    version: u32,
    intent_id: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IntentSnapshot {
    record_type: String,
    sequence: u64,
    timestamp: DateTime<Utc>,
    state: IntentState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentSummary {
    pub intent_id: String,
    pub objective: String,
    pub workspace: String,
    pub session_id: String,
    pub status: IntentStatus,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IntentIndex {
    intents: Vec<IntentSummary>,
}

pub struct NewIntent {
    pub spec: IntentSpec,
    pub workspace: String,
    pub session_id: String,
    pub secrets: Vec<String>,
}

#[derive(Clone)]
pub struct IntentHandle {
    state: Arc<Mutex<IntentState>>,
    file_path: PathBuf,
    intents_dir: PathBuf,
    secrets: Arc<Vec<String>>,
}

impl IntentHandle {
    pub fn create(intents_dir: &Path, intent: NewIntent) -> Result<Self> {
        handle::create(intents_dir, intent)
    }

    pub fn open(intents_dir: &Path, intent_id: &str, secrets: Vec<String>) -> Result<Self> {
        handle::open(intents_dir, intent_id, secrets)
    }

    pub fn snapshot(&self) -> Result<IntentState> {
        self.state
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| AppError::Intent("Active intent state is unavailable".to_string()))
    }

    pub fn amend(&self, spec: &IntentSpec) -> Result<()> {
        self.update(|state| {
            state.amend(spec);
            Ok(())
        })
    }

    pub fn record_decision(&self, key: &str, value: &str) -> Result<()> {
        self.update(|state| state.record_decision(key, value))
    }

    pub fn report_progress(&self, progress: IntentProgress) -> Result<()> {
        self.update(|state| state.report_progress(progress))
    }

    pub fn finalize_success(&self) -> Result<()> {
        self.update(|state| {
            state.finalize_success();
            Ok(())
        })
    }

    pub fn pause(&self) -> Result<()> {
        self.update(|state| {
            state.pause();
            Ok(())
        })
    }

    pub fn abandon(&self) -> Result<()> {
        self.update(|state| {
            state.abandon();
            Ok(())
        })
    }

    fn update(&self, change: impl FnOnce(&mut IntentState) -> Result<()>) -> Result<()> {
        handle::update(self, change)
    }
}

pub fn list_unfinished(intents_dir: &Path, workspace: &str) -> Result<Vec<IntentSummary>> {
    let mut idx = index::load_or_rebuild_index(intents_dir)?;
    idx.intents.retain(|intent| {
        intent.workspace == workspace
            && intent.status != IntentStatus::Completed
            && intent.status != IntentStatus::Abandoned
    });
    idx.intents.sort_by_key(|intent| Reverse(intent.updated_at));
    Ok(idx.intents)
}

pub fn find_for_session(intents_dir: &Path, session_id: &str, secrets: Vec<String>) -> Result<Option<IntentHandle>> {
    let idx = index::load_or_rebuild_index(intents_dir)?;
    let Some(summary) = idx
        .intents
        .iter()
        .filter(|intent| intent.session_id == session_id)
        .max_by_key(|intent| intent.updated_at)
    else {
        return Ok(None);
    };
    IntentHandle::open(intents_dir, &summary.intent_id, secrets).map(Some)
}

pub fn workspace_id(path: &Path) -> Result<String> {
    Ok(path.canonicalize()?.display().to_string())
}

fn load_state(path: &Path, expected_id: &str) -> Result<IntentState> {
    let bytes = std::fs::read(path)?;
    let lines = committed_lines(&bytes);
    let header_line = lines
        .first()
        .ok_or_else(|| AppError::Intent("Intent file is missing its header".to_string()))?;
    let header: IntentHeader =
        serde_json::from_slice(header_line).map_err(|_| AppError::Intent("Intent header is malformed".to_string()))?;
    if header.version != INTENT_VERSION || header.intent_id != expected_id {
        return Err(AppError::Intent("Intent identity or version is invalid".to_string()));
    }
    let mut current = None;
    for (expected_sequence, line) in lines.iter().skip(1).enumerate() {
        let snapshot: IntentSnapshot = serde_json::from_slice(line)
            .map_err(|_| AppError::Intent("Intent contains a malformed committed record".to_string()))?;
        if snapshot.sequence != expected_sequence as u64 || snapshot.state.spec.id != expected_id {
            return Err(AppError::Intent("Intent record ordering is invalid".to_string()));
        }
        current = Some(snapshot.state);
    }
    current.ok_or_else(|| AppError::Intent("Intent has no committed state".to_string()))
}

fn committed_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    lines.pop();
    lines.into_iter().filter(|line| !line.is_empty()).collect()
}

fn validate_intent_id(intent_id: &str) -> Result<()> {
    if intent_id.is_empty()
        || !intent_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AppError::Intent("Invalid intent id".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("intent_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_intent(dir: &Path) -> IntentHandle {
        IntentHandle::create(
            dir,
            NewIntent {
                spec: IntentSpec::from_prompt("fix auth"),
                workspace: "/repo".to_string(),
                session_id: "session-1".to_string(),
                secrets: Vec::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn intent_round_trips_and_lists_only_unfinished_workspace_matches() {
        let dir = temp_dir();
        let handle = create_intent(&dir);
        handle.record_decision("auth.strategy", "sessions").unwrap();

        let reopened = IntentHandle::open(&dir, &handle.snapshot().unwrap().spec.id, Vec::new()).unwrap();
        assert_eq!(reopened.snapshot().unwrap().decisions.len(), 1);
        assert_eq!(list_unfinished(&dir, "/repo").unwrap().len(), 1);
        assert!(list_unfinished(&dir, "/other").unwrap().is_empty());
    }

    #[test]
    fn paused_intent_remains_recoverable_and_abandoned_intent_is_archived() {
        let dir = temp_dir();
        let handle = create_intent(&dir);
        let path = handle.file_path.clone();
        handle.pause().unwrap();
        assert_eq!(list_unfinished(&dir, "/repo").unwrap().len(), 1);

        handle.abandon().unwrap();
        assert!(path.exists());
        assert!(list_unfinished(&dir, "/repo").unwrap().is_empty());
    }

    #[test]
    fn incomplete_tail_is_ignored() {
        let dir = temp_dir();
        let handle = create_intent(&dir);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&handle.file_path)
            .unwrap()
            .write_all(b"{partial")
            .unwrap();

        assert!(IntentHandle::open(&dir, &handle.snapshot().unwrap().spec.id, Vec::new()).is_ok());
    }
}
