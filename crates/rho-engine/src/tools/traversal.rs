//! Shared filesystem traversal configuration for the fd and rg tools.

use ignore::WalkBuilder;
use ignore::types::{Types, TypesBuilder};
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const DEFAULT_TRAVERSAL_TIMEOUT_SECS: u64 = 30;

pub struct CancelOnDrop(pub Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub fn should_quit_traversal(
    cancellation: Option<&AtomicBool>,
    timed_out: &AtomicBool,
    (start, timeout): (Instant, Duration),
) -> bool {
    if let Some(c) = cancellation
        && c.load(Ordering::Relaxed)
    {
        return true;
    }
    if timed_out.load(Ordering::Relaxed) {
        return true;
    }
    if start.elapsed() >= timeout {
        timed_out.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// Builds a workspace-scoped walker: ignore rules (.gitignore, .ignore, the
/// global gitignore, .git/info/exclude) and hidden entries are respected
/// unless `include_hidden`, and symlinks are never followed.
pub fn walker_builder(search_root: &Path, include_hidden: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(search_root);
    builder
        .hidden(!include_hidden)
        .follow_links(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true);
    builder
}

/// Selects a default file-type definition (e.g. 'rust', 'py'); unknown names
/// are rejected with the existing fd/rg error phrasing.
pub fn build_type_matcher(file_type: Option<&str>) -> Result<Option<Types>, String> {
    let Some(file_type) = file_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let mut builder = TypesBuilder::new();
    builder.add_defaults().select(file_type);
    builder
        .build()
        .map(Some)
        .map_err(|_| format!("unknown type {file_type:?}; use a default type name such as 'rust', 'js', or 'py'"))
}

/// Resolves the optional `path` argument to an absolute search root,
/// rejecting paths that do not exist.
pub fn search_root(workspace: &Workspace, path: Option<&str>) -> Result<PathBuf, String> {
    let Some(raw) = path.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(workspace.root().to_path_buf());
    };
    match workspace.resolve(raw) {
        Some(root) if root.exists() => Ok(root),
        _ => Err(format!("path not found: {raw}")),
    }
}
