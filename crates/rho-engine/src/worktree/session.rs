//! Session-level `/wt` worktrees: fork the current checkout into a managed linked
//! git worktree on a new branch, carrying uncommitted and staged changes over.

use super::WORKTREE_LOCK;
use super::clone::{self, CloneKind};
use super::git::{self, Repo};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const MAX_FALLBACK_IGNORED_FILE_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorktree {
    pub path: PathBuf,
    pub branch: String,
    pub cloned_with: Option<CloneKind>,
    pub source_cleaned: bool,
    pub clean_warning: Option<String>,
}

impl SessionWorktree {
    #[must_use]
    pub fn summary(&self) -> String {
        let how = self
            .cloned_with
            .map_or_else(|| "checked out".to_string(), |k| format!("cloned via {}", k.label()));
        let change_status = if self.source_cleaned {
            "uncommitted changes moved, source checkout cleaned"
        } else {
            "uncommitted changes carried over"
        };
        let mut msg = format!(
            "Moved to worktree {} on branch {} ({how}, {change_status}).",
            self.path.display(),
            self.branch
        );
        if let Some(warn) = &self.clean_warning {
            msg.push_str(&format!(" Warning: failed to clean source checkout ({warn})."));
        }
        msg
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClearReport {
    pub removed: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, String)>,
}

/// Default `/wt` branch name: `wt/<yyyymmdd-hhmmss>`.
#[must_use]
pub fn default_session_branch() -> String {
    chrono::Local::now().format("wt/%Y%m%d-%H%M%S").to_string()
}

pub fn validate_branch_name(branch: &str) -> Result<(), String> {
    let bad_char = |c: char| c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\');
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.ends_with('/')
        || branch.contains("..")
        || branch.contains("@{")
        || branch.chars().any(bad_char)
    {
        return Err(format!("Invalid branch name: {branch}"));
    }
    Ok(())
}

fn branch_slug(branch: &str) -> String {
    let s: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(48).collect()
}

fn path_hash(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn resolve_available_path(base_dir: &Path, branch: &str, common_dir: &Path) -> PathBuf {
    let stem = format!("{}-{}", branch_slug(branch), path_hash(common_dir));
    let candidate = base_dir.join(&stem);
    if !candidate.exists() {
        return candidate;
    }
    for n in 2..=1000 {
        let next = base_dir.join(format!("{stem}-{n}"));
        if !next.exists() {
            return next;
        }
    }
    base_dir.join(format!("{stem}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]))
}

async fn branch_exists(root: &Path, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    git::run(root, &["show-ref", "--verify", "--quiet", &full], &[])
        .await
        .is_ok()
}

async fn copy_source_index(repo: &Repo, worktree_path: &Path) -> Result<(), String> {
    let src_index = repo.git_dir.join("index");
    if !src_index.is_file() {
        return Ok(());
    }
    let wt_git_dir = PathBuf::from(git::text(worktree_path, &["rev-parse", "--absolute-git-dir"]).await?);
    tokio::fs::copy(&src_index, wt_git_dir.join("index"))
        .await
        .map_err(|e| format!("could not copy git index: {e}"))?;
    // Refresh stat cache for clean files; exit code 1 is expected when files are dirty.
    let _ = git::run(worktree_path, &["update-index", "--refresh", "-q"], &[]).await;
    Ok(())
}

fn remove_non_git_entries(dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == OsStr::new(".git") {
            continue;
        }
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

fn copy_one_path(src: &Path, dst: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(src) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let _ = std::fs::remove_file(dst);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let target = std::fs::read_link(src).map_err(|e| e.to_string())?;
            super::share::symlink(&target, dst).map_err(|e| e.to_string())?;
            Ok(())
        }
        Ok(meta) if meta.is_file() => {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(src, dst).map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
            let _ = std::fs::set_permissions(dst, meta.permissions());
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let _ = std::fs::remove_file(dst);
            Ok(())
        }
        Err(err) => Err(err.to_string()),
    }
}

async fn carry_over_fallback(repo: &Repo, worktree_path: &Path) -> Result<(), String> {
    git::run(worktree_path, &["checkout", "--force", "HEAD"], &[]).await?;

    let status_out = git::run(
        &repo.top,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        &[],
    )
    .await?;
    let mut paths = Vec::new();
    let mut fields = git::nul_fields(&status_out).into_iter();
    while let Some(entry) = fields.next() {
        if entry.len() <= 3 {
            continue;
        }
        let status = &entry.as_bytes()[..2];
        paths.push(entry[3..].to_string());
        if (status.contains(&b'R') || status.contains(&b'C'))
            && let Some(orig_path) = fields.next()
            && !orig_path.is_empty()
        {
            paths.push(orig_path);
        }
    }

    let src_root = repo.top.clone();
    let dst_root = worktree_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        for rel in &paths {
            copy_one_path(&src_root.join(rel), &dst_root.join(rel))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    copy_small_ignored_files(&repo.top, worktree_path).await?;
    copy_source_index(repo, worktree_path).await
}

async fn copy_small_ignored_files(source: &Path, worktree: &Path) -> Result<(), String> {
    let out = git::run(
        source,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
        &[],
    )
    .await?;
    for name in git::nul_fields(&out) {
        if name.is_empty() || name.ends_with('/') || name.contains('/') || name == ".git" {
            continue;
        }
        let src = source.join(&name);
        let dst = worktree.join(&name);
        if dst.symlink_metadata().is_ok() {
            continue;
        }
        if let Ok(meta) = tokio::fs::symlink_metadata(&src).await
            && meta.is_file()
            && meta.len() <= MAX_FALLBACK_IGNORED_FILE_BYTES
        {
            let _ = tokio::fs::copy(&src, &dst).await;
        }
    }
    Ok(())
}

async fn materialize_worktree(repo: &Repo, worktree_path: &Path) -> Result<Option<CloneKind>, String> {
    let src = repo.top.clone();
    let dst = worktree_path.to_path_buf();
    let clone_res = tokio::task::spawn_blocking(move || clone::clone_tree(&src, &dst, &[OsStr::new(".git")]))
        .await
        .map_err(|e| e.to_string())?;

    match clone_res {
        Ok(kind) => {
            copy_source_index(repo, worktree_path).await?;
            Ok(Some(kind))
        }
        Err(_) => {
            let dst_clean = worktree_path.to_path_buf();
            tokio::task::spawn_blocking(move || remove_non_git_entries(&dst_clean))
                .await
                .map_err(|e| e.to_string())??;
            carry_over_fallback(repo, worktree_path).await?;
            Ok(None)
        }
    }
}

async fn clean_source_checkout(source_root: &Path) -> (bool, Option<String>) {
    if let Err(err) = git::run(source_root, &["reset", "--hard", "HEAD"], &[]).await {
        return (false, Some(err));
    }
    if let Err(err) = git::run(source_root, &["clean", "-fd"], &[]).await {
        return (false, Some(err));
    }
    (true, None)
}

/// Creates a managed linked worktree for `/wt` or `worktree(action="create")`.
pub async fn create_session_worktree(
    source_cwd: &Path,
    base_dir: &Path,
    branch: &str,
    clean_source: bool,
) -> Result<SessionWorktree, String> {
    validate_branch_name(branch)?;
    let repo = git::repo(source_cwd)
        .await
        .ok_or_else(|| format!("Not inside a git repository: {}", source_cwd.display()))?;
    if repo.head.is_none() {
        return Err("Cannot create a worktree before the first commit (HEAD is unborn).".to_string());
    }
    if branch_exists(&repo.top, branch).await {
        return Err(format!("Branch '{branch}' already exists; pick another name."));
    }

    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|e| format!("could not create {}: {e}", base_dir.display()))?;
    let worktree_path = resolve_available_path(base_dir, branch, &repo.common_dir);

    {
        let _guard = WORKTREE_LOCK.lock().await;
        let path_str = worktree_path.to_string_lossy().into_owned();
        git::run(
            &repo.top,
            &[
                "worktree",
                "add",
                "--no-checkout",
                "--quiet",
                "-b",
                branch,
                &path_str,
                "HEAD",
            ],
            &[],
        )
        .await?;
    }

    let cloned_with = match materialize_worktree(&repo, &worktree_path).await {
        Ok(kind) => kind,
        Err(err) => {
            let _guard = WORKTREE_LOCK.lock().await;
            let path_str = worktree_path.to_string_lossy().into_owned();
            let _ = git::run(&repo.top, &["worktree", "remove", "--force", &path_str], &[]).await;
            let _ = git::run(&repo.top, &["branch", "-D", branch], &[]).await;
            return Err(err);
        }
    };

    let (source_cleaned, clean_warning) = if clean_source {
        clean_source_checkout(&repo.top).await
    } else {
        (false, None)
    };

    let canonical_wt = tokio::fs::canonicalize(&worktree_path).await.unwrap_or(worktree_path);
    let rel_subdir = std::fs::canonicalize(source_cwd)
        .ok()
        .and_then(|src| {
            let top = std::fs::canonicalize(&repo.top).unwrap_or_else(|_| repo.top.clone());
            src.strip_prefix(&top).ok().map(Path::to_path_buf)
        })
        .filter(|rel| !rel.as_os_str().is_empty());
    let target_path = match rel_subdir {
        Some(rel) if canonical_wt.join(&rel).is_dir() => canonical_wt.join(rel),
        _ => canonical_wt,
    };

    Ok(SessionWorktree {
        path: target_path,
        branch: branch.to_string(),
        cloned_with,
        source_cleaned,
        clean_warning,
    })
}

fn parse_porcelain_worktrees(output: &str) -> Vec<ManagedWorktree> {
    let mut list = Vec::new();
    let mut current: Option<ManagedWorktree> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(prev) = current.take() {
                list.push(prev);
            }
            current = Some(ManagedWorktree {
                path: PathBuf::from(path),
                branch: None,
                head: None,
            });
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            if let Some(item) = current.as_mut() {
                item.head = Some(head.to_string());
            }
        } else if let Some(branch) = line.strip_prefix("branch ")
            && let Some(item) = current.as_mut()
        {
            item.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_string());
        }
    }
    if let Some(last) = current {
        list.push(last);
    }
    list
}

/// Lists git worktrees belonging to this repository that live under `base_dir`.
pub async fn list_managed_worktrees(repo_cwd: &Path, base_dir: &Path) -> Result<Vec<ManagedWorktree>, String> {
    let repo = git::repo(repo_cwd)
        .await
        .ok_or_else(|| format!("Not inside a git repository: {}", repo_cwd.display()))?;
    let out = git::text(&repo.top, &["worktree", "list", "--porcelain"]).await?;
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .unwrap_or_else(|_| base_dir.to_path_buf());
    Ok(parse_porcelain_worktrees(&out)
        .into_iter()
        .filter(|wt| {
            let wt_canon = std::fs::canonicalize(&wt.path).unwrap_or_else(|_| wt.path.clone());
            wt_canon.starts_with(&canonical_base)
        })
        .collect())
}

/// Removes managed worktrees under `base_dir` for this repository (skipping the current checkout).
pub async fn clear_managed_worktrees(repo_cwd: &Path, base_dir: &Path, force: bool) -> Result<ClearReport, String> {
    let repo = git::repo(repo_cwd)
        .await
        .ok_or_else(|| format!("Not inside a git repository: {}", repo_cwd.display()))?;
    let current_top = tokio::fs::canonicalize(&repo.top).await.unwrap_or(repo.top.clone());
    let managed = list_managed_worktrees(repo_cwd, base_dir).await?;
    let mut report = ClearReport::default();

    for wt in managed {
        let wt_canon = tokio::fs::canonicalize(&wt.path).await.unwrap_or(wt.path.clone());
        if wt_canon == current_top {
            report
                .skipped
                .push((wt.path, "cannot remove the currently active worktree".to_string()));
            continue;
        }
        let path_str = wt.path.to_string_lossy().into_owned();
        let args: Vec<&str> = if force {
            vec!["worktree", "remove", "--force", &path_str]
        } else {
            vec!["worktree", "remove", &path_str]
        };
        let _guard = WORKTREE_LOCK.lock().await;
        match git::run(&repo.top, &args, &[]).await {
            Ok(_) => report.removed.push(wt.path),
            Err(err) => report.skipped.push((wt.path, err)),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    async fn init_repo(root: &Path) {
        git::run(root, &["init", "-b", "main"], &[]).await.unwrap();
        git::run(root, &["config", "user.name", "Test"], &[]).await.unwrap();
        git::run(root, &["config", "user.email", "test@example.com"], &[])
            .await
            .unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(root.join("tracked.txt"), "v1\n").unwrap();
        git::run(root, &["add", "."], &[]).await.unwrap();
        git::run(root, &["commit", "-m", "init"], &[]).await.unwrap();
    }

    #[test]
    fn validate_branch_name_rejects_bad_refs() {
        assert!(validate_branch_name("feat/my-branch").is_ok());
        assert!(validate_branch_name("wt/20261124-120000").is_ok());
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("-bad").is_err());
        assert!(validate_branch_name("a..b").is_err());
        assert!(validate_branch_name("has space").is_err());
        assert!(validate_branch_name("trail/").is_err());
    }

    #[tokio::test]
    async fn create_session_worktree_carries_dirty_staged_untracked_and_env() {
        let dir = TestDir::new();
        let repo_dir = dir.join("repo");
        let wt_base = dir.join("wt");
        std::fs::create_dir_all(&repo_dir).unwrap();
        init_repo(&repo_dir).await;

        std::fs::write(repo_dir.join("tracked.txt"), "v2-dirty\n").unwrap();
        std::fs::write(repo_dir.join("staged.txt"), "staged\n").unwrap();
        git::run(&repo_dir, &["add", "staged.txt"], &[]).await.unwrap();
        std::fs::write(repo_dir.join("untracked.txt"), "new\n").unwrap();
        std::fs::write(repo_dir.join(".env"), "API_KEY=123\n").unwrap();

        let created = create_session_worktree(&repo_dir, &wt_base, "feat/wt-test", false)
            .await
            .unwrap();
        assert_eq!(created.branch, "feat/wt-test");
        assert!(!created.source_cleaned);
        assert_eq!(
            std::fs::read_to_string(created.path.join("tracked.txt")).unwrap(),
            "v2-dirty\n"
        );
        assert_eq!(
            std::fs::read_to_string(created.path.join("staged.txt")).unwrap(),
            "staged\n"
        );
        assert_eq!(
            std::fs::read_to_string(created.path.join("untracked.txt")).unwrap(),
            "new\n"
        );
        assert_eq!(
            std::fs::read_to_string(created.path.join(".env")).unwrap(),
            "API_KEY=123\n"
        );

        // Editing .env in the worktree must not mutate the source checkout.
        std::fs::write(created.path.join(".env"), "API_KEY=changed\n").unwrap();
        assert_eq!(std::fs::read_to_string(repo_dir.join(".env")).unwrap(), "API_KEY=123\n");

        let listed = list_managed_worktrees(&repo_dir, &wt_base).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].branch.as_deref(), Some("feat/wt-test"));

        let cleared = clear_managed_worktrees(&repo_dir, &wt_base, true).await.unwrap();
        assert_eq!(cleared.removed.len(), 1);
        assert!(list_managed_worktrees(&repo_dir, &wt_base).await.unwrap().is_empty());
    }
}
