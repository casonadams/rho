//! File changes rho makes itself: servers are told through `workspace/didChangeWatchedFiles`, and
//! each file's changes are counted so late diagnostics can tell they went stale.

use super::client::file_uri;
use super::manager::LspManager;
use super::support::{lock, log_failure};
use crate::paths::canonical_or_parent;
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

/// How many times rho changed each file, and how many late-diagnostics waits are running. A file
/// whose count was dropped to stay within [`MAX_EPOCH_FILES`] reads as changed since any earlier stamp.
#[derive(Default)]
pub(super) struct Epochs {
    files: Mutex<HashMap<PathBuf, u64>>,
    pub(super) follow_ups: AtomicUsize,
}

impl LspManager {
    /// Tells every server whose root holds a path that rho wrote it (or, for a path that no longer
    /// exists, deleted it).
    pub fn files_changed(&self, paths: &[PathBuf]) {
        self.announce(paths, file_change);
    }

    /// Announces files rho just rewrote and gives each server that has one open its new contents and
    /// a save, so its diagnostics follow the file instead of the text it held before.
    pub async fn refresh_after_write(&self, paths: &[PathBuf]) {
        let paths: Vec<PathBuf> = paths.iter().map(|path| canonical_or_parent(path)).collect();
        self.bump_epochs(&paths);
        for client in self.running_clients() {
            let inside: Vec<&PathBuf> = paths.iter().filter(|path| path.starts_with(&client.root)).collect();
            let mut unopened = Vec::new();
            let mut open = Vec::new();
            for path in inside {
                if client.is_open(path).await {
                    open.push(path);
                } else {
                    unopened.push((file_uri(path), file_change(path)));
                }
            }
            log_failure("document sync", client.sync_open_documents_from_disk().await);
            log_failure("watched file change", client.did_change_watched_files(&unopened));
            for path in open {
                if let Ok(text) = tokio::fs::read_to_string(path).await {
                    let _ = client.did_save(path, &text).await;
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
        lock(&self.epochs.files)
            .get(&canonical_or_parent(path))
            .copied()
            .unwrap_or(0)
    }

    fn bump_epochs(&self, paths: &[PathBuf]) {
        let mut epochs = lock(&self.epochs.files);
        for path in paths {
            *epochs.entry(path.clone()).or_default() += 1;
            while epochs.len() > MAX_EPOCH_FILES {
                let Some(stale) = epochs.keys().find(|key| *key != path).cloned() else {
                    break;
                };
                epochs.remove(&stale);
            }
        }
    }

    fn announce(&self, paths: &[PathBuf], kind: impl Fn(&Path) -> FileChange) {
        let paths: Vec<PathBuf> = paths.iter().map(|path| canonical_or_parent(path)).collect();
        self.bump_epochs(&paths);
        for client in self.running_clients() {
            let changes: Vec<(String, FileChange)> = paths
                .iter()
                .filter(|path| path.starts_with(&client.root))
                .map(|path| (file_uri(path), kind(path)))
                .collect();
            log_failure("watched file change", client.did_change_watched_files(&changes));
        }
    }
}

/// `Changed` for a file that still exists, `Deleted` for one that is gone.
fn file_change(path: &Path) -> FileChange {
    match path.exists() {
        true => FileChange::Changed,
        false => FileChange::Deleted,
    }
}

const MAX_EPOCH_FILES: usize = 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::test_server::{FakeOptions, fake_manager};
    use rho_test_support::TestDir;

    #[tokio::test(start_paused = true)]
    async fn the_change_counts_kept_per_file_are_bounded() {
        let dir = TestDir::new();
        let (manager, _) = fake_manager(dir.as_ref(), FakeOptions::default());
        let paths: Vec<PathBuf> = (0..MAX_EPOCH_FILES + 20)
            .map(|index| dir.as_ref().join(format!("f{index}.rs")))
            .collect();
        manager.files_changed(&paths);
        assert!(lock(&manager.epochs.files).len() <= MAX_EPOCH_FILES);
        let last = paths.last().unwrap();
        assert_eq!(manager.change_count(last), 1, "the files just bumped are kept");
    }
}
