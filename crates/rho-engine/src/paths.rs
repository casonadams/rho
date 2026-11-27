use std::path::{Path, PathBuf};

/// `path` with symlinks resolved, or `path` unchanged when it cannot be resolved.
pub(crate) fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// `path` with symlinks resolved, so one file always maps to the same server root and document; a
/// file that does not exist yet resolves through its parent directory.
pub(crate) fn canonical_or_parent(path: &Path) -> PathBuf {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_or_self_keeps_unresolvable_paths() {
        let missing = Path::new("definitely/missing/rho-path");
        assert_eq!(canonical_or_self(missing), missing);
    }

    #[test]
    fn canonical_or_parent_resolves_a_missing_file_through_its_parent() {
        let dir = rho_test_support::TestDir::new();
        let canonical_dir = dir.canonicalize().unwrap();
        assert_eq!(
            canonical_or_parent(&dir.join("not-yet.rs")),
            canonical_dir.join("not-yet.rs")
        );
    }
}
