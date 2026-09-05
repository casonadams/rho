use std::path::{Path, PathBuf};

pub fn is_confined_target(canonical: &Path, base_dir: &Path) -> bool {
    if canonical.components().any(|c| c.as_os_str() == ".git") {
        return false;
    }
    let canonical_base = base_dir.canonicalize().unwrap_or_else(|_| base_dir.to_path_buf());
    let repo_root = super::super::instructions::find_repo_root(base_dir).and_then(|r| r.canonicalize().ok());

    let in_workspace =
        canonical.starts_with(&canonical_base) || repo_root.as_ref().is_some_and(|r| canonical.starts_with(r));

    let in_home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .and_then(|h| PathBuf::from(h).canonicalize().ok())
        .is_some_and(|h| canonical.starts_with(&h));

    in_workspace || in_home
}
