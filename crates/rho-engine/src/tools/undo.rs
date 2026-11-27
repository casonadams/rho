//! Undo for the changes `edit`, `write`, and the `lsp` rename, code action and format actions make:
//! each tool records a file's contents before and after its change, and `undo` restores the earlier
//! contents while the file still holds what the tool wrote.

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
    /// Directories the change created for a new file, innermost first.
    created_dirs: Vec<PathBuf>,
    /// Insertion order across all files, so the globally oldest revision can be evicted.
    seq: u64,
}

impl Revision {
    fn bytes(&self) -> usize {
        self.before.as_ref().map_or(0, Vec::len) + self.after.len()
    }
}

/// What the newest revision of a file undoes.
struct Undoing {
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    created_dirs: Vec<PathBuf>,
}

/// The file's contents before a change: `Some(None)` when it does not exist yet, `None` when it
/// could not be read, in which case the change must not be recorded (undo would delete the file).
pub async fn read_before(path: &Path) -> Option<Option<Vec<u8>>> {
    read_current(path).await.ok()
}

/// `None` when the file does not exist; other read failures are errors, not a missing file.
async fn read_current(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The changes the agent's tools made to each file this session, newest last.
#[derive(Clone, Default)]
pub struct EditHistory(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    files: HashMap<PathBuf, Vec<Revision>>,
    next_seq: u64,
}

impl EditHistory {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn record(&self, path: &Path, before: Option<Vec<u8>>, after: &[u8]) {
        self.record_with_dirs(path, before, after, Vec::new());
    }

    /// Like [`Self::record`], for a change that created `created_dirs` (innermost first) to hold a
    /// new file; undoing the creation removes the ones that are left empty.
    pub fn record_with_dirs(&self, path: &Path, before: Option<Vec<u8>>, after: &[u8], created_dirs: Vec<PathBuf>) {
        let mut state = self.state();
        let seq = state.next_seq;
        state.next_seq += 1;
        let revisions = state.files.entry(path.to_path_buf()).or_default();
        revisions.push(Revision {
            before,
            after: after.to_vec(),
            created_dirs,
            seq,
        });
        if revisions.len() > MAX_REVISIONS_PER_FILE {
            revisions.remove(0);
        }
        trim_to_budget(&mut state.files);
    }

    /// The newest revision's contents when the file still holds what the tool wrote.
    fn peek_if_current(&self, path: &Path, current: Option<&[u8]>) -> Result<Undoing, String> {
        let state = self.state();
        let last = state
            .files
            .get(path)
            .and_then(|revisions| revisions.last())
            .ok_or("no edit or write by this session's tools to undo for this file")?;
        match current {
            Some(bytes) if bytes == last.after.as_slice() => Ok(Undoing {
                before: last.before.clone(),
                after: last.after.clone(),
                created_dirs: last.created_dirs.clone(),
            }),
            None => Err("the file no longer exists (it was removed or renamed after the last edit or write); nothing was changed"
                .into()),
            Some(_) => Err("the file changed after the last edit or write (by the user, bash, or a formatter); undo would discard that change, so read the file and edit it instead"
                .into()),
        }
    }

    /// Drops the newest revision once it was undone; returns how many remain.
    fn pop(&self, path: &Path) -> usize {
        let mut state = self.state();
        let Some(revisions) = state.files.get_mut(path) else {
            return 0;
        };
        revisions.pop();
        let remaining = revisions.len();
        if remaining == 0 {
            state.files.remove(path);
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
        let Some(oldest) = files
            .iter()
            .filter_map(|(path, revisions)| revisions.first().map(|revision| (revision.seq, path)))
            .min()
            .map(|(_, path)| path.clone())
        else {
            return;
        };
        if let Some(revisions) = files.get_mut(&oldest) {
            revisions.remove(0);
            if revisions.is_empty() {
                files.remove(&oldest);
            }
        }
    }
}

/// Removes the directories `write` created, innermost first, stopping at the first one that is
/// not empty or cannot be removed; returns how many went.
async fn remove_empty_dirs(dirs: &[PathBuf]) -> usize {
    let mut removed = 0;
    for dir in dirs {
        if tokio::fs::remove_dir(dir).await.is_err() {
            break;
        }
        removed += 1;
    }
    removed
}

pub struct UndoTool {
    base_dir: crate::tools::WorkspaceDir,
    history: EditHistory,
    lsp: Option<Arc<crate::lsp::LspManager>>,
}

impl UndoTool {
    pub fn new(base_dir: impl AsRef<Path>, history: EditHistory) -> Self {
        Self {
            base_dir: crate::tools::WorkspaceDir::new(base_dir.as_ref()),
            history,
            lsp: None,
        }
    }

    pub fn with_workspace_dir(mut self, base_dir: crate::tools::WorkspaceDir) -> Self {
        self.base_dir = base_dir;
        self
    }

    /// Tells the language servers about files an undo restores or deletes.
    pub fn with_lsp(mut self, lsp: Option<Arc<crate::lsp::LspManager>>) -> Self {
        self.lsp = lsp;
        self
    }

    async fn undo(&self, clean_path: &str) -> Result<String, String> {
        let path =
            resolve_tool_path(&Workspace::new(self.base_dir.get()), clean_path, "undo").map_err(|e| e.content)?;
        let message = self.restore(&path, clean_path).await?;
        if let Some(lsp) = &self.lsp {
            lsp.refresh_after_write(&[path]).await;
        }
        Ok(message)
    }

    async fn restore(&self, path: &Path, clean_path: &str) -> Result<String, String> {
        let current = read_current(path)
            .await
            .map_err(|error| format!("reading {clean_path}: {error}; nothing was changed"))?;
        let Undoing {
            before,
            after,
            created_dirs,
        } = self.history.peek_if_current(path, current.as_deref())?;
        let Some(before) = before else {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| format!("removing {clean_path}: {error}; nothing was changed"))?;
            let removed = remove_empty_dirs(&created_dirs).await;
            return Ok(format!(
                "Deleted {clean_path}, which write had created{}{}",
                match removed {
                    0 => String::new(),
                    n => format!(" (and {})", crate::tools::listing::count_noun(n, "empty directory")),
                },
                self.more(path)
            ));
        };
        atomic_write(path, &before)
            .await
            .map_err(|error| format!("writing {clean_path}: {error}; nothing was changed"))?;
        let mut message = format!(
            "Restored {clean_path} to its contents before the last change{}",
            self.more(path)
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
