//! Git repositories for subagent workspace tests.

use rho_test_support::TestDir;
use std::path::{Path, PathBuf};

/// Runs `git` in `root` with a fixed identity and no global config; the test fails if git does.
pub fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.email=t@e", "-c", "user.name=t"])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

/// Initializes a repository on `main` in `dir` and commits `files` with the content `v1`.
pub fn committed_repo(dir: &TestDir, files: &[&str]) -> PathBuf {
    let root = dir.canonicalize().unwrap();
    git(&root, &["init", "--quiet", "-b", "main"]);
    for file in files {
        std::fs::write(root.join(file), "v1").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "init"]);
    root
}
