//! Small helpers shared across the LSP modules.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, continuing with the data a panicking holder left behind.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `path` with symlinks resolved, so one file always maps to the same server root and document; a
/// file that does not exist yet resolves through its parent directory.
pub(super) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
        .unwrap_or_else(|| path.to_path_buf())
}
