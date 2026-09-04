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

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() {
            let head_file = git_dir.join("HEAD");
            if let Ok(head_content) = std::fs::read_to_string(head_file) {
                let trimmed = head_content.trim();
                if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                    return Some(branch.to_string());
                }
            }
            break;
        } else if git_dir.is_file()
            && let Ok(content) = std::fs::read_to_string(git_dir)
            && let Some(gitdir_path) = content.trim().strip_prefix("gitdir:")
        {
            let gitdir = PathBuf::from(gitdir_path.trim());
            let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
            let head_file = resolved.join("HEAD");
            if let Ok(head_content) = std::fs::read_to_string(head_file) {
                let trimmed = head_content.trim();
                if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                    return Some(branch.to_string());
                }
            }
            break;
        }
        curr = dir.parent();
    }

    let mut cmd = std::process::Command::new("git");
    cmd.arg("branch").arg("--show-current");
    cmd.current_dir(cwd);
    if let Ok(output) = cmd.output()
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}
