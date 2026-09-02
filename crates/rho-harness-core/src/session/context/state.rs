//! Compactor sidecar state on disk and the free [`compaction_input`] helper
//! that decides what to carry forward from a previous compaction.
//!
//! Extracted from `session/context.rs` during the file-length refactor.

use std::io::Write;
use std::path::PathBuf;

use rig::memory::MemoryError;
use rig::message::Message;
use serde::{Deserialize, Serialize};

use super::CodingArtifact;
use super::CodingCompactor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CompactionState {
    pub(super) version: u32,
    pub(super) absorbed_hashes: Vec<String>,
    pub(super) artifact: String,
}

impl CodingCompactor {
    pub(super) fn state_path(&self) -> PathBuf {
        self.session().file_path.with_extension("context.json")
    }

    pub(super) fn load_state(&self) -> Result<Option<CompactionState>, MemoryError> {
        match std::fs::read(self.state_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| MemoryError::Internal("stored compaction state is malformed".to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(MemoryError::backend(error)),
        }
    }

    pub(super) fn persist_state(&self, state: &CompactionState) -> Result<(), MemoryError> {
        let path = self.state_path();
        let temporary = path.with_extension(format!("context.{}.tmp", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(state)
            .map_err(|_| MemoryError::Internal("compaction state serialization failed".to_string()))?;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(MemoryError::backend)?;
        file.write_all(&bytes).map_err(MemoryError::backend)?;
        file.sync_data().map_err(MemoryError::backend)?;
        std::fs::rename(temporary, path).map_err(MemoryError::backend)
    }
}

pub(super) struct CompactionInputParams<'a> {
    pub(super) stored: Option<&'a CompactionState>,
    pub(super) carry_over: Option<&'a CodingArtifact>,
    pub(super) evicted: &'a [Message],
    pub(super) hashes: &'a [String],
}

pub(super) fn compaction_input<'a>(params: CompactionInputParams<'a>) -> (Option<String>, &'a [Message], Vec<String>) {
    if let Some(carry) = params.carry_over {
        let absorbed = params
            .stored
            .filter(|state| state.artifact == carry.as_str())
            .map_or_else(
                || params.hashes.to_vec(),
                |state| {
                    let mut combined = state.absorbed_hashes.clone();
                    combined.extend(params.hashes.iter().cloned());
                    combined
                },
            );
        return (Some(carry.as_str().to_string()), params.evicted, absorbed);
    }
    if let Some(state) = params.stored
        && params.hashes.starts_with(&state.absorbed_hashes)
    {
        let suffix = params.evicted.get(state.absorbed_hashes.len()..).unwrap_or_default();
        return (Some(state.artifact.clone()), suffix, params.hashes.to_vec());
    }
    (None, params.evicted, params.hashes.to_vec())
}
