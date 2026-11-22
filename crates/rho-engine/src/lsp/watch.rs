//! File changes rho makes itself: servers are told through `workspace/didChangeWatchedFiles`, and
//! each file's changes are counted so late diagnostics can tell they went stale.

use super::client::file_uri;
use super::manager::{LspManager, canonical, lock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    Created,
    Changed,
    Deleted,
}

impl FileChange {
    /// The `FileChangeType` the protocol numbers it with.
    pub fn code(self) -> u8 {
        match self {
            Self::Created => 1,
            Self::Changed => 2,
            Self::Deleted => 3,
        }
    }
}

/// How many times rho changed each file, and how many late-diagnostics waits are running.
#[derive(Default)]
pub(super) struct Epochs {
    files: Mutex<HashMap<PathBuf, u64>>,
    pub(super) follow_ups: AtomicUsize,
}

impl LspManager {
    /// Tells every server whose root holds a path that rho wrote it (or, for a path that no longer
    /// exists, deleted it).
    pub fn files_changed(&self, paths: &[PathBuf]) {
        self.announce(paths, |path| match path.exists() {
            true => FileChange::Changed,
            false => FileChange::Deleted,
        });
    }

    /// Announces files rho just rewrote and gives each server that has one open its new contents and
    /// a save, so its diagnostics follow the file instead of the text it held before.
    pub async fn refresh_after_write(&self, paths: &[PathBuf]) {
        self.files_changed(paths);
        for client in self.running_clients() {
            let _ = client.sync_open_documents_from_disk().await;
            for path in paths
                .iter()
                .map(|path| canonical(path))
                .filter(|path| path.starts_with(&client.root))
            {
                if let Ok(text) = tokio::fs::read_to_string(&path).await {
                    let _ = client.did_save(&path, &text).await;
                }
            }
        }
    }

    /// Like [`LspManager::files_changed`], for files that did not exist before.
    pub fn files_created(&self, paths: &[PathBuf]) {
        self.announce(paths, |_| FileChange::Created);
    }

    /// How many times rho has changed `path` through [`LspManager::files_changed`] or
    /// [`LspManager::files_created`].
    pub fn change_count(&self, path: &Path) -> u64 {
        lock(&self.epochs.files).get(&canonical(path)).copied().unwrap_or(0)
    }

    fn announce(&self, paths: &[PathBuf], kind: impl Fn(&Path) -> FileChange) {
        let paths: Vec<PathBuf> = paths.iter().map(|path| canonical(path)).collect();
        {
            let mut epochs = lock(&self.epochs.files);
            for path in &paths {
                *epochs.entry(path.clone()).or_default() += 1;
            }
        }
        for client in self.running_clients() {
            let changes: Vec<(String, FileChange)> = paths
                .iter()
                .filter(|path| path.starts_with(&client.root))
                .map(|path| (file_uri(path), kind(path)))
                .collect();
            let _ = client.did_change_watched_files(&changes);
        }
    }
}
