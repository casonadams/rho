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
