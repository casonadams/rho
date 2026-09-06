#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

/// Resolves tool paths against the engine's fixed workspace root.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    excluded: Vec<PathBuf>,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self::with_exclusions(root, std::iter::empty::<&Path>())
    }

    pub fn with_exclusions<I, P>(root: impl AsRef<Path>, exclusions: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let root_path = root.as_ref();
        let root = canonicalize_path(root_path);
        let excluded = exclusions
            .into_iter()
            .map(|path| canonicalize_path(path.as_ref()))
            .collect();
        Self { root, excluded }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, raw_path: &str) -> Option<PathBuf> {
        let clean = raw_path.trim().trim_matches(['\'', '"']);
        if clean.is_empty() {
            return None;
        }
        if clean == "~" {
            return std::env::var("HOME").ok().map(PathBuf::from);
        }
        if let Some(rest) = clean.strip_prefix("~/") {
            return std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(rest));
        }
        let path = Path::new(clean);
        Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        })
    }

    /// Uses normalized canonical paths so targets that do not exist yet are checked safely.
    pub fn is_within(&self, raw_path: &str) -> bool {
        let Some(candidate) = self.resolve(raw_path) else {
            return false;
        };
        canonicalize_path(&candidate).starts_with(&self.root)
    }

    pub fn is_protected(&self, raw_path: &str) -> bool {
        let Some(candidate) = self.resolve(raw_path) else {
            return false;
        };
        let canonical = canonicalize_path(&candidate);
        canonical
            .strip_prefix(&self.root)
            .ok()
            .is_some_and(|relative| relative.components().any(|c| c.as_os_str() == ".git"))
    }

    pub fn is_excluded(&self, raw_path: &str) -> bool {
        let Some(candidate) = self.resolve(raw_path) else {
            return false;
        };
        let canonical = canonicalize_path(&candidate);
        self.excluded
            .iter()
            .any(|path| canonical == *path || canonical.starts_with(path))
    }

    pub fn can_mutate(&self, raw_path: &str) -> bool {
        self.is_within(raw_path) && !self.is_protected(raw_path) && !self.is_excluded(raw_path)
    }

    pub fn list_files(&self, max_files: usize) -> Vec<String> {
        list_relative_files(&self.root, max_files)
    }

    pub async fn list_files_async(&self, max_files: usize) -> Vec<String> {
        list_relative_files_async(&self.root, max_files).await
    }
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
    (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>),
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

async fn step_async_entry(
    root: &Path,
    entry: tokio::fs::DirEntry,
    (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>),
) {
    let file_name = entry.file_name();
    if !is_ignored_directory_or_file(&file_name.to_string_lossy()) {
        process_async_entry(root, entry, (dirs, files)).await;
    }
}

async fn drain_async_entries(
    (root, entries): (&Path, &mut tokio::fs::ReadDir),
    (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>),
    max_files: usize,
) {
    while let Ok(Some(entry)) = entries.next_entry().await {
        step_async_entry(root, entry, (dirs, files)).await;
        if files.len() >= max_files {
            break;
        }
    }
}

async fn drain_async_dir(
    (root, current): (&Path, &Path),
    (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>),
    max_files: usize,
) {
    if let Ok(mut entries) = tokio::fs::read_dir(current).await {
        drain_async_entries((root, &mut entries), (dirs, files), max_files).await;
    }
}

pub async fn list_relative_files_async(root: &Path, max_files: usize) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(current) = dirs.pop() {
        drain_async_dir((root, &current), (&mut dirs, &mut files), max_files).await;
        if files.len() >= max_files {
            break;
        }
    }
    files.sort();
    files
}

fn process_sync_entry(root: &Path, path: PathBuf, (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>)) {
    if path.is_dir() {
        dirs.push(path);
    } else if path.is_file()
        && let Some(rel) = to_forward_slash_rel(root, &path)
    {
        files.push(rel);
    }
}

fn step_sync_entry(root: &Path, entry: std::fs::DirEntry, (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>)) {
    let file_name = entry.file_name();
    if !is_ignored_directory_or_file(&file_name.to_string_lossy()) {
        process_sync_entry(root, entry.path(), (dirs, files));
    }
}

fn drain_sync_dir(
    (root, current): (&Path, &Path),
    (dirs, files): (&mut Vec<PathBuf>, &mut Vec<String>),
    max_files: usize,
) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        step_sync_entry(root, entry, (dirs, files));
        if files.len() >= max_files {
            break;
        }
    }
}

pub fn list_relative_files(root: &Path, max_files: usize) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(current) = dirs.pop() {
        drain_sync_dir((root, &current), (&mut dirs, &mut files), max_files);
        if files.len() >= max_files {
            break;
        }
    }
    files.sort();
    files
}

fn canonicalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let mut non_existing = Vec::new();
    let mut current = path;
    while let Some(parent) = current.parent() {
        if let Some(file_name) = current.file_name() {
            non_existing.push(file_name);
        }
        if let Ok(canonical_parent) = parent.canonicalize() {
            let mut resolved = canonical_parent;
            for component in non_existing.into_iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }
        current = parent;
    }
    path.to_path_buf()
}
