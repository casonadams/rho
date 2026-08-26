//! Intent index management.
//!
//! The index is a small JSON file (`index.json` next to the per-intent
//! `.jsonl` files) that lists every known intent as an [`IntentSummary`]. It
//! is a *cache*: every read path tries to load it, falling back to
//! [`rebuild_index`] when the file is missing or malformed. Every successful
//! persistence in [`super::handle`] calls [`update_index`] so the cache stays
//! eventually consistent with the per-intent files.

use super::super::lifecycle::IntentState;
use super::{IntentIndex, IntentSummary};
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use std::path::Path;

pub(super) fn summary(state: &IntentState, updated_at: DateTime<Utc>) -> IntentSummary {
    IntentSummary {
        intent_id: state.spec.id.clone(),
        objective: state.spec.objective.clone(),
        workspace: state.workspace.clone(),
        session_id: state.session_id.clone(),
        status: state.status,
        updated_at,
    }
}

pub(super) fn load_or_rebuild_index(intents_dir: &Path) -> Result<IntentIndex> {
    if !intents_dir.exists() {
        return Ok(IntentIndex::default());
    }
    let index_path = intents_dir.join("index.json");
    if let Ok(bytes) = std::fs::read(&index_path)
        && let Ok(index) = serde_json::from_slice(&bytes)
    {
        return Ok(index);
    }
    rebuild_index(intents_dir)
}

pub(super) fn rebuild_index(intents_dir: &Path) -> Result<IntentIndex> {
    let mut index = IntentIndex::default();
    for entry in std::fs::read_dir(intents_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(intent_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Ok(state) = super::load_state(&path, intent_id) {
            let updated_at = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(|_| Utc::now());
            index.intents.push(summary(&state, updated_at));
        }
    }
    write_index(intents_dir, &index)?;
    Ok(index)
}

pub(super) fn update_index(intents_dir: &Path, updated: IntentSummary) -> Result<()> {
    let mut index = load_or_rebuild_index(intents_dir)?;
    index.intents.retain(|intent| intent.intent_id != updated.intent_id);
    index.intents.push(updated);
    write_index(intents_dir, &index)
}

pub(super) fn write_index(intents_dir: &Path, index: &IntentIndex) -> Result<()> {
    std::fs::create_dir_all(intents_dir)?;
    let path = intents_dir.join("index.json");
    let temporary = intents_dir.join("index.json.tmp");
    let bytes =
        serde_json::to_vec(index).map_err(|_| AppError::Intent("Intent index serialization failed".to_string()))?;
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}
