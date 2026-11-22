//! Undo for the changes `edit` and `write` make: each tool records a file's contents before and
//! after its change, and `undo` restores the earlier contents while the file still holds what the
//! tool wrote.

use crate::tools::atomic::atomic_write;
use crate::tools::edit::preview::changed_regions;
use crate::tools::engine_tool::TypedTool;
use crate::tools::types::{ToolResult, resolve_tool_path};
pub use rho_harness_core::args::UndoArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::{Workspace, clean_path_arg};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub const MAX_REVISIONS_PER_FILE: usize = 20;

struct Revision {
    /// `None` when the change created the file.
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

/// The changes the agent's tools made to each file this session, newest last.
#[derive(Clone, Default)]
pub struct EditHistory(Arc<Mutex<HashMap<PathBuf, Vec<Revision>>>>);

impl EditHistory {
    fn files(&self) -> MutexGuard<'_, HashMap<PathBuf, Vec<Revision>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn record(&self, path: &Path, before: Option<Vec<u8>>, after: &[u8]) {
        let mut files = self.files();
        let revisions = files.entry(path.to_path_buf()).or_default();
        revisions.push(Revision {
            before,
            after: after.to_vec(),
        });
        if revisions.len() > MAX_REVISIONS_PER_FILE {
            revisions.remove(0);
        }
    }

    fn pop_if_current(&self, path: &Path, current: Option<&[u8]>) -> Result<(Revision, usize), String> {
        let mut files = self.files();
        let revisions = files
            .get_mut(path)
            .filter(|revisions| !revisions.is_empty())
            .ok_or("no edit or write by this session's tools to undo for this file")?;
        let last = revisions.last().map(|revision| revision.after.as_slice());
        if current != last {
            return Err(
                "the file changed after the last edit or write (by the user, bash, or a formatter); undo would discard that change, so read the file and edit it instead"
                    .into(),
            );
        }
        let revision = revisions.pop().ok_or("nothing to undo")?;
        Ok((revision, revisions.len()))
    }
}

pub struct UndoTool {
    base_dir: PathBuf,
    history: EditHistory,
}

impl UndoTool {
    pub fn new(base_dir: impl AsRef<Path>, history: EditHistory) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            history,
        }
    }

    async fn undo(&self, clean_path: &str) -> Result<String, String> {
        let path = resolve_tool_path(&Workspace::new(&self.base_dir), clean_path, "undo").map_err(|e| e.content)?;
        let current = tokio::fs::read(&path).await.ok();
        let (revision, remaining) = self.history.pop_if_current(&path, current.as_deref())?;
        let more = match remaining {
            0 => String::new(),
            n => format!(" ({n} earlier change(s) can still be undone)"),
        };
        let Some(before) = revision.before else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|error| format!("removing {clean_path}: {error}"))?;
            return Ok(format!("Deleted {clean_path}, which write had created{more}"));
        };
        atomic_write(&path, &before)
            .await
            .map_err(|error| format!("writing {clean_path}: {error}"))?;
        let mut message = format!("Restored {clean_path} to its contents before the last change{more}");
        if let (Ok(old), Ok(new)) = (std::str::from_utf8(&revision.after), std::str::from_utf8(&before)) {
            let regions = changed_regions(old, new);
            if !regions.is_empty() {
                message.push('\n');
                message.push_str(&regions);
            }
        }
        Ok(message)
    }
}

#[async_trait::async_trait]
impl TypedTool for UndoTool {
    type Args = UndoArgs;
    const NAME: &'static str = "undo";

    async fn run(&self, args: UndoArgs) -> Result<ToolResult, AppError> {
        Ok(match self.undo(clean_path_arg(&args.path)).await {
            Ok(message) => ToolResult::success(message),
            Err(message) => ToolResult::error(message),
        })
    }
}

#[cfg(test)]
mod tests;
