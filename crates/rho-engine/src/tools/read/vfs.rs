use std::path::Path;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub enum VfsUri {
    Diff { target: String },
    Conflict { file: Option<String> },
    Github { item_type: String, id: String },
}

pub fn parse_vfs_uri(uri: &str) -> Option<VfsUri> {
    let trimmed = uri.trim();
    if let Some(target) = trimmed.strip_prefix("diff://") {
        return Some(VfsUri::Diff {
            target: if target.is_empty() {
                "HEAD".to_string()
            } else {
                target.to_string()
            },
        });
    }
    if let Some(rest) = trimmed.strip_prefix("conflict://") {
        let file = if rest.is_empty() { None } else { Some(rest.to_string()) };
        return Some(VfsUri::Conflict { file });
    }
    if let Some(rest) = trimmed.strip_prefix("gh://") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return Some(VfsUri::Github {
                item_type: parts[0].to_string(),
                id: parts[1].to_string(),
            });
        }
    }
    if let Some(id) = trimmed.strip_prefix("pr://") {
        return Some(VfsUri::Github {
            item_type: "pr".to_string(),
            id: id.to_string(),
        });
    }
    if let Some(id) = trimmed.strip_prefix("issue://") {
        return Some(VfsUri::Github {
            item_type: "issue".to_string(),
            id: id.to_string(),
        });
    }
    None
}

pub fn resolve_diff(base_dir: &Path, target: &str) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(base_dir).arg("diff");
    if target != "HEAD" && !target.is_empty() {
        cmd.arg(target);
    }
    let output = cmd.output().map_err(|e| format!("Failed to invoke git diff: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff error: {err}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        Ok(format!("[No differences found for diff://{target}]"))
    } else {
        Ok(stdout)
    }
}

pub fn resolve_conflicts(base_dir: &Path, file: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(base_dir)
        .args(["diff", "--name-only", "--diff-filter=U"]);
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to check git conflicts: {e}"))?;
    let unmerged = String::from_utf8_lossy(&output.stdout);
    let conflict_files: Vec<&str> = unmerged.lines().filter(|s| !s.trim().is_empty()).collect();

    if let Some(target_file) = file {
        let full_path = base_dir.join(target_file);
        if !full_path.exists() {
            return Err(format!("Conflict file not found: {target_file}"));
        }
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("Failed to read conflict file {target_file}: {e}"))?;
        Ok(content)
    } else if conflict_files.is_empty() {
        Ok("[No active merge conflicts in workspace]".to_string())
    } else {
        Ok(format!(
            "Active merge conflicts found in {} file(s):\n{}",
            conflict_files.len(),
            conflict_files.join("\n")
        ))
    }
}

pub fn resolve_vfs(base_dir: &Path, uri: &VfsUri) -> Result<String, String> {
    match uri {
        VfsUri::Diff { target } => resolve_diff(base_dir, target),
        VfsUri::Conflict { file } => resolve_conflicts(base_dir, file.as_deref()),
        VfsUri::Github { item_type, id } => Ok(format!(
            "[Virtual GitHub resource: {item_type} #{id}. Connect GitHub token for full live fetch.]"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vfs_uri() {
        assert_eq!(
            parse_vfs_uri("diff://HEAD~1"),
            Some(VfsUri::Diff {
                target: "HEAD~1".to_string()
            })
        );
        assert_eq!(
            parse_vfs_uri("conflict://src/main.rs"),
            Some(VfsUri::Conflict {
                file: Some("src/main.rs".to_string())
            })
        );
        assert_eq!(
            parse_vfs_uri("pr://102"),
            Some(VfsUri::Github {
                item_type: "pr".to_string(),
                id: "102".to_string()
            })
        );
        assert_eq!(
            parse_vfs_uri("issue://42"),
            Some(VfsUri::Github {
                item_type: "issue".to_string(),
                id: "42".to_string()
            })
        );
        assert_eq!(parse_vfs_uri("src/main.rs"), None);
    }

    #[test]
    fn test_resolve_vfs_diff_and_conflicts() {
        let cwd = std::env::current_dir().unwrap();
        let diff_res = resolve_vfs(
            &cwd,
            &VfsUri::Diff {
                target: "HEAD".to_string(),
            },
        );
        assert!(diff_res.is_ok());

        let conflict_res = resolve_vfs(&cwd, &VfsUri::Conflict { file: None });
        assert!(conflict_res.is_ok());
    }
}
