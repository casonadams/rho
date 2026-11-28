//! Restores the working directory a session was recorded in and describes the outcome.
//! Shared by the headless startup path and the frontend-neutral session operations.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::Result;
use rho_harness_core::session::SessionManager;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceRestore {
    Unchanged,
    Restored,
    Missing(PathBuf),
    Failed { path: PathBuf, error: String },
}

impl WorkspaceRestore {
    pub(crate) fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub(crate) fn note(&self) -> Option<String> {
        match self {
            Self::Unchanged | Self::Restored => None,
            Self::Missing(path) => Some(format!("previous workspace {} no longer exists", path.display())),
            Self::Failed { path, error } => Some(format!("could not restore workspace {}: {error}", path.display())),
        }
    }
}

pub(crate) fn restore_saved_cwd(
    sessions_dir: &Path,
    session_id: &str,
    change_dir: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<WorkspaceRestore> {
    let saved = SessionManager::cwd_for_session(sessions_dir, session_id)?;
    Ok(restore_workspace(saved, change_dir))
}

pub(crate) fn restore_workspace(
    saved: Option<PathBuf>,
    change_dir: impl FnOnce(&Path) -> io::Result<()>,
) -> WorkspaceRestore {
    let Some(saved) = saved else {
        return WorkspaceRestore::Unchanged;
    };
    if !saved.is_dir() {
        return WorkspaceRestore::Missing(saved);
    }
    match change_dir(&saved) {
        Ok(()) => WorkspaceRestore::Restored,
        Err(err) => WorkspaceRestore::Failed {
            path: saved,
            error: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    #[test]
    fn restore_workspace_maps_the_saved_cwd_to_an_outcome() {
        let scratch = TestDir::new();
        let existing = scratch.to_path_buf();
        let missing = scratch.join("gone");
        let ok = |_: &Path| -> io::Result<()> { Ok(()) };
        let denied = |_: &Path| -> io::Result<()> { Err(io::Error::other("permission denied")) };

        assert_eq!(restore_workspace(None, ok), WorkspaceRestore::Unchanged);
        assert_eq!(
            restore_workspace(Some(existing.clone()), ok),
            WorkspaceRestore::Restored
        );
        assert_eq!(
            restore_workspace(Some(missing.clone()), ok),
            WorkspaceRestore::Missing(missing)
        );
        assert_eq!(
            restore_workspace(Some(existing.clone()), denied),
            WorkspaceRestore::Failed {
                path: existing,
                error: "permission denied".to_string(),
            }
        );
    }

    #[test]
    fn restore_saved_cwd_reads_the_recorded_cwd_before_restoring() {
        let dir = TestDir::new();
        let sessions_dir = dir.join("sessions");
        let worktree = dir.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        let ok = |_: &Path| -> io::Result<()> { Ok(()) };

        assert_eq!(
            restore_saved_cwd(&sessions_dir, "missing-sess", ok).unwrap(),
            WorkspaceRestore::Unchanged
        );

        SessionManager::record_session_for_cwd(&sessions_dir, &worktree, "sess-wt").unwrap();
        let canonical_wt = std::fs::canonicalize(&worktree).unwrap();
        assert_eq!(
            restore_saved_cwd(&sessions_dir, "sess-wt", ok).unwrap(),
            WorkspaceRestore::Restored
        );

        std::fs::remove_dir_all(&worktree).unwrap();
        assert_eq!(
            restore_saved_cwd(&sessions_dir, "sess-wt", ok).unwrap(),
            WorkspaceRestore::Missing(canonical_wt)
        );
    }

    #[test]
    fn restore_saved_cwd_surfaces_an_unreadable_sessions_dir() {
        let dir = TestDir::new();
        let sessions_file = dir.join("not-a-dir");
        std::fs::write(&sessions_file, "").unwrap();
        let ok = |_: &Path| -> io::Result<()> { Ok(()) };

        assert!(restore_saved_cwd(&sessions_file, "sess-wt", ok).is_err());
    }
}
