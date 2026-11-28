//! The single write path for session records: plan, write, and replay run in one spawned task,
//! so dropping the caller cannot leave the file and the sequence counter apart.

use super::SessionManager;
use super::format::{
    Durability, SessionRecord, StoreState, append_records, load_file, refresh_stale_messages, replay_record,
};
use super::fs::{run_blocking, session_error};
use super::tree::TreeNodeData;
use crate::error::{AppError, Result};
use std::path::Path;
use std::sync::Arc;

/// The records one append writes, and the value its caller receives after they are applied.
/// No records means nothing is written.
pub(crate) struct Planned<T> {
    pub(crate) records: Vec<SessionRecord>,
    pub(crate) output: T,
}

pub(crate) fn tree_node(sequence: u64, session_id: &str, node: TreeNodeData) -> SessionRecord {
    SessionRecord::TreeNode {
        sequence,
        session_id: session_id.to_string(),
        node,
    }
}

impl SessionManager {
    /// Plans against the current state, then writes and applies the planned records.
    pub(crate) async fn append<T: Send + 'static>(
        &self,
        durability: Durability,
        plan: impl FnOnce(&StoreState, &str) -> Result<Planned<T>> + Send + 'static,
    ) -> Result<T> {
        let state = Arc::clone(&self.state);
        let path = self.file_path.clone();
        let session_id = self.session_id.clone();
        let step = tokio::spawn(async move {
            let mut state = state.lock().await;
            let planned = plan(&state, &session_id)?;
            if !planned.records.is_empty() {
                commit(&mut state, &path, &session_id, planned.records, durability).await?;
            }
            Ok(planned.output)
        });
        step.await
            .map_err(|e| session_error(format!("session append task failed: {e}")))?
    }
}

/// Writes `records`, then replays them into `state`. A failed write may have reached the file
/// in part or in full, so on failure `state` is rebuilt from the file.
async fn commit(
    state: &mut StoreState,
    path: &Path,
    session_id: &str,
    records: Vec<SessionRecord>,
    durability: Durability,
) -> Result<()> {
    let written = append_records(path, &records, durability).await;
    let applied = written.and_then(|()| replay_all(state, records, session_id));
    match applied {
        Ok(()) => Ok(()),
        Err(cause) => Err(resync(state, path, session_id, cause).await),
    }
}

fn replay_all(state: &mut StoreState, records: Vec<SessionRecord>, session_id: &str) -> Result<()> {
    let mut messages_stale = false;
    for record in records {
        replay_record(state, record, session_id, &mut messages_stale)?;
    }
    refresh_stale_messages(state, &mut messages_stale);
    Ok(())
}

async fn resync(state: &mut StoreState, path: &Path, session_id: &str, cause: AppError) -> AppError {
    let (path, session_id) = (path.to_path_buf(), session_id.to_string());
    match run_blocking(move || load_file(&path, &session_id)).await {
        Ok(fresh) => {
            *state = fresh;
            cause
        }
        Err(reload) => session_error(format!("{cause}; reloading the session also failed: {reload}")),
    }
}
