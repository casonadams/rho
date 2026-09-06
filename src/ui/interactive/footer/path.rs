use std::path::{Path, PathBuf};

pub fn abbreviate_home(cwd: &Path, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return cwd.display().to_string();
    };
    if cwd == home {
        return "~".to_string();
    }
    if let Ok(rel) = cwd.strip_prefix(home) {
        let rel_str = rel.to_string_lossy();
        if rel_str.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rel_str}");
    }
    cwd.display().to_string()
}

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
