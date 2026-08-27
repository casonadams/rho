use std::path::{Path, PathBuf};

/// Resolves tool paths against the engine's fixed workspace root.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| root.as_ref().to_path_buf());
        Self { root }
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

    /// Uses the nearest existing ancestor so paths for new files are checked too.
    pub fn is_within(&self, raw_path: &str) -> bool {
        let Some(candidate) = self.resolve(raw_path) else {
            return false;
        };
        existing_ancestor(&candidate).is_some_and(|path| path.starts_with(&self.root))
    }

    pub fn is_protected(&self, raw_path: &str) -> bool {
        let Some(candidate) = self.resolve(raw_path) else {
            return false;
        };
        candidate
            .strip_prefix(&self.root)
            .ok()
            .is_some_and(|relative| relative.components().any(|c| c.as_os_str() == ".git"))
    }
}

fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut ancestor = path;
    loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            return Some(canonical);
        }
        ancestor = ancestor.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_and_absolute_paths_from_fixed_root() {
        let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let workspace = Workspace::new(&root);
        assert_eq!(workspace.resolve("src/lib.rs"), Some(canonical_root.join("src/lib.rs")));
        assert_eq!(workspace.resolve(" "), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protects_git_and_rejects_escape() {
        let root = std::env::temp_dir().join(format!("workspace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let workspace = Workspace::new(&root);
        assert!(workspace.is_protected(".git/config"));
        assert!(!workspace.is_within("../outside.txt"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
