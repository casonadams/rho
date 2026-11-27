//! Per-agent git worktrees: every agent works in its own checkout of a snapshot of the tree that
//! started it, and its changes reach that tree only when its parent merges them.

pub mod clone;
pub(crate) mod git;
mod lease;
mod merge;
mod review;
pub mod session;
mod share;
mod sweep;

use crate::digest::sha256_hex;
use crate::locking::lock;
use chrono::{DateTime, Utc};
pub use clone::CloneKind;
pub use lease::{WorktreeLease, settle_abandoned};
pub use merge::MergeOutcome;
use serde::{Deserialize, Serialize};
pub use session::{
    ClearReport, ManagedWorktree, SessionWorktree, clear_managed_worktrees, create_session_worktree,
    default_session_branch, list_managed_worktrees, validate_branch_name,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
pub use sweep::EXPIRE_AFTER;
use tokio::sync::Mutex;

const MAX_DIFF_CHARS: usize = 30_000;
/// How many review findings `diff` and `merge` results list.
pub const REVIEW_FINDINGS_SHOWN: usize = 30;

/// Serializes `git worktree add/remove`, which update shared repository state.
static WORKTREE_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub id: String,
    /// What the worktree is for, e.g. `subagent performanceAudit` or `workflow refine`.
    pub label: String,
    pub path: PathBuf,
    pub branch: String,
    /// The snapshot commit the worktree started from.
    pub base: String,
    /// The working tree it was created from, where its changes are merged by default.
    pub source: PathBuf,
    pub common_dir: PathBuf,
    pub created: DateTime<Utc>,
    /// The session that created it and is responsible for merging or discarding it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub status: char,
    pub path: String,
}

impl FileChange {
    #[must_use]
    pub fn label(&self) -> String {
        match self.status {
            'A' => format!("{} (added)", self.path),
            'D' => format!("{} (deleted)", self.path),
            _ => self.path.clone(),
        }
    }
}

/// Where worktree checkouts are kept: one directory per repository under `dir`, outside every project.
/// Checkouts are disposable; each worktree's branch and its metadata in `.git/rho/worktrees` are the record.
#[derive(Debug, Clone)]
pub struct WorktreeStore {
    dir: PathBuf,
    /// Working directory -> (repository's checkout directory, git common dir); saves a git call per lookup.
    repos: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, (PathBuf, PathBuf)>>>,
}

impl PartialEq for WorktreeStore {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir
    }
}

impl Eq for WorktreeStore {}

fn repo_key(common_dir: &Path) -> String {
    let project = common_dir
        .parent()
        .and_then(Path::file_name)
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().into_owned());
    let hash = sha256_hex(common_dir.to_string_lossy().as_bytes())[..8].to_string();
    format!("{project}-{hash}")
}

fn slug(label: &str) -> String {
    let slug: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.chars().take(40).collect()
}

impl WorktreeStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            repos: std::sync::Arc::default(),
        }
    }

    #[must_use]
    pub fn default_location() -> Self {
        Self::new(checkout_root())
    }

    async fn repo_dir(&self, root: &Path) -> Result<(PathBuf, PathBuf), String> {
        let cached = lock(&self.repos).get(root).cloned();
        if let Some(found) = cached {
            return Ok(found);
        }
        let common = git::repo(root)
            .await
            .ok_or_else(|| format!("{} is not in a git repository", root.display()))?
            .common_dir;
        let found = (self.dir.join(repo_key(&common)), common);
        lock(&self.repos).insert(root.to_path_buf(), found.clone());
        Ok(found)
    }

    /// Creates a worktree on a new branch from a snapshot of `source`'s working tree, owned by session
    /// `owner`; `None` outside git. A failure after `git worktree add` removes what was created.
    pub async fn create(&self, source: &Path, label: &str, owner: Option<&str>) -> Result<Option<Worktree>, String> {
        let Some(repo) = git::repo(source).await else {
            return Ok(None);
        };
        let base = git::snapshot_repo(&repo).await?;
        let (source, common_dir) = (repo.top, repo.common_dir);
        self.sweep_once(&common_dir).await;
        private_dir(&self.dir).await?;
        let repo_dir = self.dir.join(repo_key(&common_dir));
        tokio::fs::create_dir_all(&repo_dir)
            .await
            .map_err(|e| format!("could not create {}: {e}", repo_dir.display()))?;
        let repo_dir = tokio::fs::canonicalize(&repo_dir)
            .await
            .map_err(|e| format!("could not resolve {}: {e}", repo_dir.display()))?;
        let id = format!("{}-{}", slug(label), &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let worktree = Worktree {
            path: repo_dir.join(&id),
            branch: format!("rho/wt/{id}"),
            id,
            label: label.to_string(),
            base,
            source: source.clone(),
            common_dir,
            created: Utc::now(),
            owner: owner.map(str::to_string),
        };
        {
            let _guard = WORKTREE_LOCK.lock().await;
            let path = worktree.path.to_string_lossy().into_owned();
            let args = [
                "worktree",
                "add",
                "--no-checkout",
                "--quiet",
                "-b",
                &worktree.branch,
                &path,
                &worktree.base,
            ];
            git::run(&source, &args, &[]).await?;
        }
        let finished = async {
            let src = source.clone();
            let dst = worktree.path.clone();
            let cloned =
                tokio::task::spawn_blocking(move || clone::clone_tree(&src, &dst, &[std::ffi::OsStr::new(".git")]))
                    .await
                    .map_err(|e| e.to_string())?;
            if cloned.is_ok() {
                git::run(&worktree.path, &["reset", "--mixed", "--quiet", "HEAD"], &[]).await?;
            } else {
                let dst_clean = worktree.path.clone();
                tokio::task::spawn_blocking(move || session::remove_non_git_entries(&dst_clean))
                    .await
                    .map_err(|e| e.to_string())??;
                git::run(&worktree.path, &["reset", "--hard", "--quiet", "HEAD"], &[]).await?;
                share::share_ignored(&source, &worktree.path, &worktree.common_dir).await?;
            }
            worktree.save().await
        }
        .await;
        if let Err(error) = finished {
            let _ = worktree.remove().await;
            return Err(error);
        }
        Ok(Some(worktree))
    }

    /// The worktree `id` of the repository containing `root`, recreating its checkout from its branch
    /// if the temp directory was cleared; `None` once it is merged, discarded, or expired.
    pub async fn find(&self, root: &Path, id: &str) -> Option<Worktree> {
        let (_, common_dir) = self.repo_dir(root).await.ok()?;
        self.load(&meta_dir(&common_dir).join(format!("{id}.json"))).await
    }

    /// Every worktree of the repository containing `root`, oldest first.
    pub async fn list(&self, root: &Path) -> Vec<Worktree> {
        let Ok((_, common_dir)) = self.repo_dir(root).await else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for path in meta_files(&common_dir).await {
            found.extend(self.load(&path).await);
        }
        found.sort_by_key(|w| w.created);
        found
    }

    /// The worktrees created by session `owner`, read from their metadata without touching checkouts.
    pub async fn owned(&self, root: &Path, owner: &str) -> Vec<Worktree> {
        let Ok((_, common_dir)) = self.repo_dir(root).await else {
            return Vec::new();
        };
        let mut owned = Vec::new();
        for path in meta_files(&common_dir).await {
            owned.extend(read_meta(&path).await.filter(|w| w.owner.as_deref() == Some(owner)));
        }
        owned.sort_by_key(|w| w.created);
        owned
    }

    async fn load(&self, meta: &Path) -> Option<Worktree> {
        let worktree = read_meta(meta).await?;
        if worktree.path.is_dir() {
            return Some(worktree);
        }
        match worktree.restore().await {
            Ok(()) => Some(worktree),
            Err(_) => {
                if !sweep::branch_exists(&worktree.common_dir, &worktree.branch).await {
                    let _ = worktree.remove().await;
                }
                None
            }
        }
    }
}

pub(crate) fn meta_dir(common_dir: &Path) -> PathBuf {
    common_dir.join("rho").join("worktrees")
}

pub(crate) async fn meta_files(common_dir: &Path) -> Vec<PathBuf> {
    let Ok(mut entries) = tokio::fs::read_dir(meta_dir(common_dir)).await else {
        return Vec::new();
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
    files
}

pub(crate) async fn read_meta(path: &Path) -> Option<Worktree> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&text).ok()
}

/// `<temp>/rho-worktrees-<uid>`, private to the current user: temp directories may be shared (`/tmp`).
fn checkout_root() -> PathBuf {
    #[cfg(unix)]
    let name = format!("rho-worktrees-{}", unsafe { libc::getuid() });
    #[cfg(not(unix))]
    let name = "rho-worktrees".to_string();
    std::env::temp_dir().join(name)
}

/// Creates `dir` if needed and refuses one another user owns.
async fn private_dir(dir: &Path) -> Result<(), String> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = tokio::fs::symlink_metadata(dir).await.map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
            return Err(format!(
                "refusing to use {}: not a directory owned by you",
                dir.display()
            ));
        }
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|e| format!("could not restrict {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// A worktree after a round: its committed head and changes, or removed because it changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub head: String,
    pub changes: Vec<FileChange>,
    pub removed: bool,
}

impl Worktree {
    fn meta_path(&self) -> PathBuf {
        meta_dir(&self.common_dir).join(format!("{}.json", self.id))
    }

    async fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let path = self.meta_path();
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
        }
        rho_harness_core::fs::write_atomic_async(&path, text.into_bytes(), 0o600)
            .await
            .map_err(|e| format!("could not save worktree metadata: {e}"))
    }

    fn git_dir_arg(&self) -> String {
        format!("--git-dir={}", self.common_dir.display())
    }

    /// Checks the branch out again after the temp directory holding the checkout was cleared.
    async fn restore(&self) -> Result<(), String> {
        let git_dir = self.git_dir_arg();
        let path = self.path.to_string_lossy().into_owned();
        {
            let _guard = WORKTREE_LOCK.lock().await;
            if self.path.is_dir() {
                return Ok(());
            }
            git::run(&self.common_dir, &[&git_dir, "worktree", "prune"], &[]).await?;
            if let Some(parent) = self.path.parent() {
                private_dir(parent.parent().unwrap_or(parent)).await?;
                tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
            }
            let args = [git_dir.as_str(), "worktree", "add", "--quiet", &path, &self.branch];
            git::run(&self.common_dir, &args, &[]).await?;
        }
        share::share_ignored(&self.source, &self.path, &self.common_dir).await
    }

    /// Commits the worktree after a round and removes it if it changed nothing.
    pub async fn settle(&self) -> Result<Settled, String> {
        let (head, changes) = self.changes().await?;
        let removed = changes.is_empty();
        if removed {
            self.remove().await?;
        }
        Ok(Settled { head, changes, removed })
    }

    /// Commits everything in the worktree to its branch; returns the new head commit.
    pub async fn commit(&self) -> Result<String, String> {
        git::run(&self.path, &["add", "--all", "--", "."], &[]).await?;
        let tree = git::text(&self.path, &["write-tree"]).await?;
        let heads = git::text(&self.path, &["rev-parse", "HEAD", "HEAD^{tree}"]).await?;
        let (head, head_tree) = heads.split_once('\n').ok_or("could not read the worktree's HEAD")?;
        if tree == head_tree.trim() {
            return Ok(head.to_string());
        }
        let commit = git::commit_tree(&self.path, &tree, Some(head), &format!("rho: {}", self.label)).await?;
        git::run(&self.path, &["update-ref", "HEAD", &commit], &[]).await?;
        Ok(commit)
    }

    /// Commits the worktree, then lists what it changed since it started; returns the head commit too.
    pub async fn changes(&self) -> Result<(String, Vec<FileChange>), String> {
        let head = self.commit().await?;
        let paths = git::changed_paths(&self.path, &self.base, &head).await?;
        let changes = paths
            .into_iter()
            .map(|(status, path)| FileChange { status, path })
            .collect();
        Ok((head, changes))
    }

    /// The code review of the change, then a stat summary and patch of everything the worktree
    /// changed, clipped for display.
    pub async fn diff(&self) -> Result<String, String> {
        let head = self.commit().await?;
        let review = self.review_head(&head).await?;
        let stat = git::text(&self.path, &["diff", "--stat", &self.base, &head]).await?;
        let patch = git::run(&self.path, &["diff", &self.base, &head], &[]).await?;
        let patch = String::from_utf8_lossy(&patch);
        let mut out = format!("{}\n\n{stat}\n\n", review.render(REVIEW_FINDINGS_SHOWN));
        let total = patch.chars().count();
        out.extend(patch.chars().take(MAX_DIFF_CHARS));
        if total > MAX_DIFF_CHARS {
            let _ = write!(
                out,
                "\n[... {} more characters; read the files in {}]",
                total - MAX_DIFF_CHARS,
                self.path.display()
            );
        }
        Ok(out)
    }

    /// Deletes the checkout, its branch, and its metadata; parts already gone are skipped.
    pub async fn remove(&self) -> Result<(), String> {
        let path = self.path.to_string_lossy().into_owned();
        let git_dir = self.git_dir_arg();
        {
            let _guard = WORKTREE_LOCK.lock().await;
            let args: &[&str] = if self.path.exists() {
                &[&git_dir, "worktree", "remove", "--force", &path]
            } else {
                &[&git_dir, "worktree", "prune"]
            };
            git::run(&self.common_dir, args, &[]).await?;
        }
        let branch_ref = format!("refs/heads/{}", self.branch);
        git::run(&self.common_dir, &[&git_dir, "update-ref", "-d", &branch_ref], &[]).await?;
        match tokio::fs::remove_file(self.meta_path()).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("could not delete worktree metadata: {e}"))
            }
            _ => Ok(()),
        }
    }

    /// Merges the worktree's changes into `target` (a working tree of the same repository) and removes
    /// the worktree; conflicting files are left in `target` with conflict markers.
    pub async fn merge_into(&self, target: &Path) -> Result<MergeOutcome, String> {
        let head = self.commit().await?;
        self.merge_head_into(&head, target).await
    }

    /// Like `merge_into`, for a worktree already committed at `head`.
    pub async fn merge_head_into(&self, head: &str, target: &Path) -> Result<MergeOutcome, String> {
        let outcome = merge::merge(target, &self.base, head).await?;
        self.remove().await?;
        Ok(outcome)
    }

    /// One line for reports: id, branch, location, and what changed.
    #[must_use]
    pub fn summary(&self, changes: &[FileChange]) -> String {
        let files: Vec<String> = changes.iter().map(FileChange::label).collect();
        format!(
            "worktree `{}` (branch {}, {}): {} file(s) changed: {}",
            self.id,
            self.branch,
            self.path.display(),
            changes.len(),
            files.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::git::test_repo::{create, git, write};
    use super::*;

    #[tokio::test]
    async fn worktrees_start_from_uncommitted_state_and_keep_changes_on_their_branch() {
        let (dir, root) = create(&[("a.rs", "v1"), (".gitignore", "/target\nnode_modules/\n")]);
        write(&root, "a.rs", "uncommitted");
        write(&root, "target/debug/app", "bin");
        write(&root, "node_modules/x/index.js", "js");
        let store = WorktreeStore::new(dir.path().join("store"));

        let wt = store.create(&root, "subagent perf audit", None).await.unwrap().unwrap();

        assert!(wt.id.starts_with("subagent-perf-audit-"), "{}", wt.id);
        assert!(!wt.path.starts_with(&root));
        assert_eq!(std::fs::read_to_string(wt.path.join("a.rs")).unwrap(), "uncommitted");
        assert_eq!(
            std::fs::read_to_string(wt.path.join("target/debug/app")).unwrap(),
            "bin"
        );
        assert!(wt.path.join("node_modules/x/index.js").exists());
        write(&wt.path, "a.rs", "agent");
        write(&wt.path, "b.rs", "new");

        let (_, changes) = wt.changes().await.unwrap();

        let labels: Vec<String> = changes.iter().map(FileChange::label).collect();
        assert_eq!(labels, ["a.rs", "b.rs (added)"], "shared ignored dirs are not changes");
        assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), "uncommitted");
        assert_eq!(git(&root, &["status", "--porcelain"]), "M a.rs");
        assert_eq!(store.find(&root, &wt.id).await, Some(wt.clone()));
        assert_eq!(
            store.list(&wt.path).await,
            std::slice::from_ref(&wt),
            "found from inside the worktree too"
        );

        wt.remove().await.unwrap();

        assert!(!wt.path.exists());
        assert!(store.list(&root).await.is_empty());
        assert!(!git(&root, &["branch", "--list"]).contains("rho/wt/"));
    }

    #[tokio::test]
    async fn outside_git_there_is_no_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorktreeStore::new(dir.path().join("store"));
        assert_eq!(store.create(dir.path(), "x", None).await.unwrap(), None);
    }
}
