use super::super::instructions::find_repo_root;
use crate::paths::canonical_or_self;
use std::path::Path;

pub fn is_confined_target(canonical: &Path, base_dir: &Path) -> bool {
    if canonical.components().any(|c| c.as_os_str() == ".git") {
        return false;
    }
    if crate::permission::path::is_credential_file(canonical) {
        return false;
    }
    let canonical_base = canonical_or_self(base_dir);
    let repo_root = find_repo_root(base_dir).and_then(|r| r.canonicalize().ok());

    let in_workspace =
        canonical.starts_with(&canonical_base) || repo_root.as_ref().is_some_and(|r| canonical.starts_with(r));

    let in_home = rho_harness_core::config::home_dir()
        .and_then(|h| h.canonicalize().ok())
        .is_some_and(|h| canonical.starts_with(&h));

    in_workspace || in_home
}
