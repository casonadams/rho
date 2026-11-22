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
/// Total bytes of file contents the history keeps across all files; the oldest revisions go first.
pub const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

struct Revision {
    /// `None` when the change created the file.
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

impl Revision {
    fn bytes(&self) -> usize {
        self.before.as_ref().map_or(0, Vec::len) + self.after.len()
    }
}

/// The file's contents before a change: `Some(None)` when it does not exist yet, `None` when it
/// could not be read, in which case the change must not be recorded (undo would delete the file).
pub async fn read_before(path: &Path) -> Option<Option<Vec<u8>>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(None),
        Err(_) => None,
    }
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
        trim_to_budget(&mut files);
    }

    /// The newest revision's contents when the file still holds what the tool wrote.
    fn peek_if_current(&self, path: &Path, current: Option<&[u8]>) -> Result<(Option<Vec<u8>>, Vec<u8>), String> {
        let files = self.files();
        let last = files
            .get(path)
            .and_then(|revisions| revisions.last())
            .ok_or("no edit or write by this session's tools to undo for this file")?;
        match current {
            Some(bytes) if bytes == last.after.as_slice() => Ok((last.before.clone(), last.after.clone())),
            None => Err("the file no longer exists (it was removed or renamed after the last edit or write); nothing was changed"
                .into()),
            Some(_) => Err("the file changed after the last edit or write (by the user, bash, or a formatter); undo would discard that change, so read the file and edit it instead"
                .into()),
        }
    }

    /// Drops the newest revision once it was undone; returns how many remain.
    fn pop(&self, path: &Path) -> usize {
        let mut files = self.files();
        let Some(revisions) = files.get_mut(path) else {
            return 0;
        };
        revisions.pop();
        let remaining = revisions.len();
        if remaining == 0 {
            files.remove(path);
        }
        remaining
    }
}

fn trim_to_budget(files: &mut HashMap<PathBuf, Vec<Revision>>) {
    loop {
        let total: usize = files.values().flatten().map(Revision::bytes).sum();
        if total <= MAX_HISTORY_BYTES {
            return;
        }
        let Some(largest) = files
            .iter()
            .filter(|(_, revisions)| revisions.len() > 1)
            .max_by_key(|(_, revisions)| revisions.iter().map(Revision::bytes).sum::<usize>())
            .map(|(path, _)| path.clone())
        else {
            return;
        };
        if let Some(revisions) = files.get_mut(&largest) {
            revisions.remove(0);
        }
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
        let (before, after) = self.history.peek_if_current(&path, current.as_deref())?;
        let Some(before) = before else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|error| format!("removing {clean_path}: {error}; nothing was changed"))?;
            return Ok(format!(
                "Deleted {clean_path}, which write had created{}",
                self.more(&path)
            ));
        };
        atomic_write(&path, &before)
            .await
            .map_err(|error| format!("writing {clean_path}: {error}; nothing was changed"))?;
        let mut message = format!(
            "Restored {clean_path} to its contents before the last change{}",
            self.more(&path)
        );
        if let (Ok(old), Ok(new)) = (std::str::from_utf8(&after), std::str::from_utf8(&before)) {
            let regions = changed_regions(old, new);
            if !regions.is_empty() {
                message.push('\n');
                message.push_str(&regions);
            }
        }
        Ok(message)
    }

    /// Forgets the undone revision and names how many earlier ones remain.
    fn more(&self, path: &Path) -> String {
        match self.history.pop(path) {
            0 => String::new(),
            n => format!(" ({n} earlier change(s) can still be undone)"),
        }
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
