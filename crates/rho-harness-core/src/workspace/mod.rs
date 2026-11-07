#[cfg(test)]
mod tests;

use std::path::{Component, Path, PathBuf};

/// Resolves tool paths against the engine's fixed workspace root.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: canonicalize_path(root.as_ref()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, raw_path: &str) -> Option<PathBuf> {
        resolve_path(raw_path, &self.root)
    }

    pub fn list_files(&self, max_files: usize) -> Vec<String> {
        list_relative_files(&self.root, max_files)
    }

    pub async fn list_files_async(&self, max_files: usize) -> Vec<String> {
        list_relative_files_async(&self.root, max_files).await
    }
}

pub fn clean_path_arg(raw: &str) -> &str {
    raw.trim().trim_matches(['\'', '"'])
}

pub fn expand_home(path: &str) -> String {
    let Ok(home) = std::env::var("HOME") else {
        return path.to_string();
    };
    if path == "~" || path == "$HOME" {
        return home;
    }
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("$HOME/")) {
        Some(rest) => format!("{home}/{rest}"),
        None => path.to_string(),
    }
}

pub fn resolve_path(raw: &str, base: &Path) -> Option<PathBuf> {
    let clean = clean_path_arg(raw);
    if clean.is_empty() {
        return None;
    }
    let expanded = expand_home(clean);
    let path = Path::new(&expanded);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    Some(canonicalize_path(&absolute))
}

pub fn canonicalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |dir| dir.join(path))
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => {
                resolved.push(other);
                if let Ok(real) = resolved.canonicalize() {
                    resolved = real;
                }
            }
        }
    }
    resolved
}

fn is_ignored_directory_or_file(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist" | "build")
}

fn to_forward_slash_rel(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

async fn process_async_entry(
    root: &Path,
    entry: tokio::fs::DirEntry,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<String>,
) {
    let Ok(metadata) = entry.metadata().await else {
        return;
    };
    let path = entry.path();
    if metadata.is_dir() {
        dirs.push(path);
    } else if metadata.is_file()
        && let Some(rel) = to_forward_slash_rel(root, &path)
    {
        files.push(rel);
    }
}

async fn step_async_entry(root: &Path, entry: tokio::fs::DirEntry, dirs: &mut Vec<PathBuf>, files: &mut Vec<String>) {
    let file_name = entry.file_name();
    if !is_ignored_directory_or_file(&file_name.to_string_lossy()) {
        process_async_entry(root, entry, dirs, files).await;
    }
}

async fn drain_async_entries(
    root: &Path,
    entries: &mut tokio::fs::ReadDir,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<String>,
    max_files: usize,
) {
    while let Ok(Some(entry)) = entries.next_entry().await {
        step_async_entry(root, entry, dirs, files).await;
        if files.len() >= max_files {
            break;
        }
    }
}

async fn drain_async_dir(
    root: &Path,
    current: &Path,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<String>,
    max_files: usize,
) {
    if let Ok(mut entries) = tokio::fs::read_dir(current).await {
        drain_async_entries(root, &mut entries, dirs, files, max_files).await;
    }
}

pub async fn list_relative_files_async(root: &Path, max_files: usize) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(current) = dirs.pop() {
        drain_async_dir(root, &current, &mut dirs, &mut files, max_files).await;
        if files.len() >= max_files {
            break;
        }
    }
    files.sort();
    files
}

fn process_sync_entry(root: &Path, path: PathBuf, dirs: &mut Vec<PathBuf>, files: &mut Vec<String>) {
    if path.is_dir() {
        dirs.push(path);
    } else if path.is_file()
        && let Some(rel) = to_forward_slash_rel(root, &path)
    {
        files.push(rel);
    }
}

fn step_sync_entry(root: &Path, entry: std::fs::DirEntry, dirs: &mut Vec<PathBuf>, files: &mut Vec<String>) {
    let file_name = entry.file_name();
    if !is_ignored_directory_or_file(&file_name.to_string_lossy()) {
        process_sync_entry(root, entry.path(), dirs, files);
    }
}

fn drain_sync_dir(root: &Path, current: &Path, dirs: &mut Vec<PathBuf>, files: &mut Vec<String>, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        step_sync_entry(root, entry, dirs, files);
        if files.len() >= max_files {
            break;
        }
    }
}

pub fn list_relative_files(root: &Path, max_files: usize) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(current) = dirs.pop() {
        drain_sync_dir(root, &current, &mut dirs, &mut files, max_files);
        if files.len() >= max_files {
            break;
        }
    }
    files.sort();
    files
}
