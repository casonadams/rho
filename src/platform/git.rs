//! Current git branch of a working directory, read from `.git` without spawning a process when possible.

use std::path::{Path, PathBuf};

fn branch_from_head_file(head_file: &Path) -> Option<String> {
    let head_content = std::fs::read_to_string(head_file).ok()?;
    head_content.trim().strip_prefix("ref: refs/heads/").map(str::to_string)
}

fn branch_from_git_dir(dir: &Path, git_dir: &Path) -> Option<String> {
    let head_file = git_dir.join("HEAD");
    if git_dir.is_dir() {
        return branch_from_head_file(&head_file);
    }
    if git_dir.is_file() {
        let content = std::fs::read_to_string(git_dir).ok()?;
        let gitdir_path = content.trim().strip_prefix("gitdir:")?;
        let gitdir = PathBuf::from(gitdir_path.trim());
        let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
        return branch_from_head_file(&resolved.join("HEAD"));
    }
    None
}

fn branch_from_git_process(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("branch")
        .arg("--show-current")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

pub async fn get_git_branch_async(cwd: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || get_git_branch(&cwd))
        .await
        .ok()
        .flatten()
}

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() || git_dir.is_file() {
            if let Some(branch) = branch_from_git_dir(dir, &git_dir) {
                return Some(branch);
            }
            break;
        }
        curr = dir.parent();
    }

    branch_from_git_process(cwd)
}
