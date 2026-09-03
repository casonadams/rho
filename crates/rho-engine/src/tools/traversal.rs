//! Shared filesystem traversal configuration for the fd and rg tools.

use ignore::WalkBuilder;
use ignore::types::{Types, TypesBuilder};
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};

/// Builds a workspace-scoped walker: ignore rules (.gitignore, .ignore, the
/// global gitignore, .git/info/exclude) and hidden entries are respected
/// unless `include_hidden`, and symlinks are never followed.
pub fn walker_builder(search_root: &Path, include_hidden: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(search_root);
    builder.hidden(!include_hidden).follow_links(false);
    if include_hidden {
        builder
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false);
    } else {
        builder
            .ignore(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .parents(true);
    }
    builder
}

/// Selects a default file-type definition (e.g. 'rust', 'py'); unknown names
/// are rejected with the existing fd/rg error phrasing.
pub fn build_type_matcher(file_type: Option<&str>) -> Result<Option<Types>, String> {
    let Some(file_type) = file_type else {
        return Ok(None);
    };
    let mut builder = TypesBuilder::new();
    builder.add_defaults().select(file_type);
    builder
        .build()
        .map(Some)
        .map_err(|_| format!("unknown type {file_type:?}; use a default type name such as 'rust', 'js', or 'py'"))
}

/// Resolves the optional `path` argument to an absolute search root inside the
/// workspace, rejecting paths that escape it or do not exist.
pub fn search_root(workspace: &Workspace, path: Option<&str>) -> Result<PathBuf, String> {
    let Some(raw) = path.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(workspace.root().to_path_buf());
    };
    if !workspace.is_within(raw) {
        return Err(format!("path {raw:?} is outside the workspace"));
    }
    match workspace.resolve(raw) {
        Some(root) if root.exists() => Ok(root),
        _ => Err(format!("path not found: {raw}")),
    }
}
