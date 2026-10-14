use std::path::Path;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub enum VfsUri {
    Diff {
        target: String,
    },
    Conflict {
        file: Option<String>,
    },
    Github {
        repo: Option<(String, String)>,
        item_type: String,
        id: String,
        is_diff: bool,
    },
}

fn parse_gh_target(rest: &str) -> Option<VfsUri> {
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [owner, repo, "pull" | "pulls", id] => Some(VfsUri::Github {
            repo: Some(((*owner).to_string(), (*repo).to_string())),
            item_type: "pr".to_string(),
            id: (*id).to_string(),
            is_diff: false,
        }),
        [owner, repo, "pull" | "pulls", id, "diff"] => Some(VfsUri::Github {
            repo: Some(((*owner).to_string(), (*repo).to_string())),
            item_type: "pr".to_string(),
            id: (*id).to_string(),
            is_diff: true,
        }),
        [owner, repo, "issues" | "issue", id] => Some(VfsUri::Github {
            repo: Some(((*owner).to_string(), (*repo).to_string())),
            item_type: "issue".to_string(),
            id: (*id).to_string(),
            is_diff: false,
        }),
        [item_type, id, ..] => Some(VfsUri::Github {
            repo: None,
            item_type: (*item_type).to_string(),
            id: (*id).to_string(),
            is_diff: false,
        }),
        _ => None,
    }
}

fn parse_pr_target(rest: &str) -> Option<VfsUri> {
    let is_diff = rest.ends_with("/diff");
    let id_part = rest.strip_suffix("/diff").unwrap_or(rest).trim();
    if id_part.is_empty() {
        return None;
    }
    Some(VfsUri::Github {
        repo: None,
        item_type: "pr".to_string(),
        id: id_part.to_string(),
        is_diff,
    })
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
        return parse_gh_target(rest);
    }
    if let Some(id) = trimmed.strip_prefix("pr://") {
        return parse_pr_target(id);
    }
    if let Some(id) = trimmed.strip_prefix("issue://") {
        let id_clean = id.trim();
        if id_clean.is_empty() {
            return None;
        }
        return Some(VfsUri::Github {
            repo: None,
            item_type: "issue".to_string(),
            id: id_clean.to_string(),
            is_diff: false,
        });
    }
    None
}

pub fn resolve_diff(base_dir: &Path, target: &str) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(base_dir).arg("diff");
    if target == "staged" || target == "--staged" || target == "--cached" {
        cmd.arg("--staged");
    } else if target != "HEAD" && !target.is_empty() {
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

#[derive(Default)]
struct ConflictState {
    hunk_index: usize,
    in_conflict: bool,
    in_base: bool,
    in_theirs: bool,
}

impl ConflictState {
    fn process_line(&mut self, line: &str, line_num: usize, out: &mut String) {
        if line.starts_with("<<<<<<<") {
            self.hunk_index += 1;
            self.in_conflict = true;
            self.in_base = false;
            self.in_theirs = false;
            out.push_str(&format!(
                "\n--- [Conflict Hunk #{} @ line {line_num}] ---\n",
                self.hunk_index
            ));
            out.push_str(&format!("[OURS] {line}\n"));
        } else if self.in_conflict && line.starts_with("|||||||") {
            self.in_base = true;
            out.push_str(&format!("[BASE] {line}\n"));
        } else if self.in_conflict && line.starts_with("=======") {
            self.in_base = false;
            self.in_theirs = true;
            out.push_str(&format!("[THEIRS] {line}\n"));
        } else if self.in_conflict && line.starts_with(">>>>>>>") {
            out.push_str(&format!("[END] {line}\n"));
            self.in_conflict = false;
            self.in_base = false;
            self.in_theirs = false;
        } else if self.in_conflict {
            let section = if self.in_theirs {
                "THEIRS"
            } else if self.in_base {
                "BASE"
            } else {
                "OURS"
            };
            out.push_str(&format!("  [{section}:{line_num}] {line}\n"));
        }
    }
}

pub fn extract_conflict_hunks(content: &str) -> String {
    let mut out = String::new();
    let mut state = ConflictState::default();

    for (i, line) in content.lines().enumerate() {
        state.process_line(line, i + 1, &mut out);
    }

    if state.hunk_index == 0 {
        content.to_string()
    } else {
        format!("Found {} conflict hunk(s):\n{out}", state.hunk_index)
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
        Ok(extract_conflict_hunks(&content))
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

fn exec_clean_stdout(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    if cfg!(test) && program == "gh" {
        return None;
    }
    let res = Command::new(program).current_dir(dir).args(args).output().ok()?;
    res.status
        .success()
        .then(|| String::from_utf8_lossy(&res.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn detect_github_repo(base_dir: &Path) -> Option<(String, String)> {
    let url = exec_clean_stdout(base_dir, "git", &["config", "--get", "remote.origin.url"])?;
    parse_github_remote(&url)
}

pub(crate) fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let path = if let Some(ssh) = url.strip_prefix("git@github.com:") {
        ssh
    } else {
        url.strip_prefix("https://github.com/")?
    };
    let clean = path.strip_suffix(".git").unwrap_or(path).trim_matches('/');
    let mut parts = clean.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn resolve_github_vfs(
    base_dir: &Path,
    repo: Option<&(String, String)>,
    item_type: &str,
    id: &str,
    is_diff: bool,
) -> Result<String, String> {
    let (owner, repo_name) = match repo {
        Some((o, r)) => (o.clone(), r.clone()),
        None => match detect_github_repo(base_dir) {
            Some((o, r)) => (o, r),
            None => {
                return Ok(format!(
                    "[Virtual GitHub resource: {item_type} #{id}. Working directory has no detectable GitHub remote.]"
                ));
            }
        },
    };

    let repo_arg = format!("{owner}/{repo_name}");
    if is_diff && let Some(text) = exec_clean_stdout(base_dir, "gh", &["pr", "diff", id, "--repo", &repo_arg]) {
        return Ok(text);
    }

    let view_cmd = if item_type == "pr" { "pr" } else { "issue" };
    if let Some(text) = exec_clean_stdout(
        base_dir,
        "gh",
        &[view_cmd, "view", id, "--repo", &repo_arg, "--comments"],
    ) {
        return Ok(text);
    }

    Ok(format!(
        "[Virtual GitHub resource: {owner}/{repo_name} {item_type} #{id}. Install/authenticate `gh` CLI or connect GitHub token for live fetch.]"
    ))
}

pub fn resolve_vfs(base_dir: &Path, uri: &VfsUri) -> Result<String, String> {
    match uri {
        VfsUri::Diff { target } => resolve_diff(base_dir, target),
        VfsUri::Conflict { file } => resolve_conflicts(base_dir, file.as_deref()),
        VfsUri::Github {
            repo,
            item_type,
            id,
            is_diff,
        } => resolve_github_vfs(base_dir, repo.as_ref(), item_type, id, *is_diff),
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
                target: "HEAD~1".into()
            })
        );
        assert_eq!(
            parse_vfs_uri("conflict://a.rs"),
            Some(VfsUri::Conflict {
                file: Some("a.rs".into())
            })
        );
        assert_eq!(
            parse_vfs_uri("pr://102"),
            Some(VfsUri::Github {
                repo: None,
                item_type: "pr".into(),
                id: "102".into(),
                is_diff: false
            })
        );
        assert_eq!(
            parse_vfs_uri("issue://42"),
            Some(VfsUri::Github {
                repo: None,
                item_type: "issue".into(),
                id: "42".into(),
                is_diff: false
            })
        );
        assert!(parse_vfs_uri("src/main.rs").is_none());
    }

    #[test]
    fn test_resolve_vfs_diff_and_conflicts() {
        let cwd = std::env::current_dir().unwrap();
        assert!(resolve_vfs(&cwd, &VfsUri::Diff { target: "HEAD".into() }).is_ok());
        assert!(resolve_vfs(&cwd, &VfsUri::Conflict { file: None }).is_ok());
    }
}
