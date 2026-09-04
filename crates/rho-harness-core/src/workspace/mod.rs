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
}

pub fn list_relative_files(root: &Path, max_files: usize) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs_to_visit = vec![root.to_path_buf()];

    while let Some(current_dir) = dirs_to_visit.pop() {
        let Ok(entries) = std::fs::read_dir(&current_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if name_str.starts_with('.')
                || name_str == "target"
                || name_str == "node_modules"
                || name_str == "dist"
                || name_str == "build"
            {
                continue;
            }

            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if path.is_file()
                && let Ok(rel) = path.strip_prefix(root)
            {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                files.push(rel_str);
                if files.len() >= max_files {
                    break;
                }
            }
        }
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
