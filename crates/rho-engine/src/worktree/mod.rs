//! Per-agent git worktrees: every agent works in its own checkout of a snapshot of the tree that
//! started it, and its changes reach that tree only when its parent merges them.

pub(crate) mod git;
mod merge;
mod share;

pub use merge::MergeOutcome;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

const MAX_DIFF_CHARS: usize = 30_000;

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

/// Where worktrees are kept: one directory per repository under `dir`, outside every project.
#[derive(Debug, Clone)]
pub struct WorktreeStore {
    dir: PathBuf,
    /// Working directory -> (repository's worktree directory, git common dir); saves a git call per lookup.
    repos: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, (PathBuf, PathBuf)>>>,
}

impl PartialEq for WorktreeStore {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir
    }
}

impl Eq for WorktreeStore {}

fn repo_key(common_dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let project = common_dir
        .parent()
        .and_then(Path::file_name)
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().into_owned());
    let digest = Sha256::digest(common_dir.to_string_lossy().as_bytes());
    let hash: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
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

    /// `<rho config dir>/worktrees`.
    #[must_use]
    pub fn default_location() -> Option<Self> {
        rho_harness_core::config::config_dir().map(|dir| Self::new(dir.join("worktrees")))
    }

    async fn repo_dir(&self, root: &Path) -> Result<(PathBuf, PathBuf), String> {
        let cached = self
            .repos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(root)
            .cloned();
        if let Some(found) = cached {
            return Ok(found);
        }
        let common = git::repo(root)
            .await
            .ok_or_else(|| format!("{} is not in a git repository", root.display()))?
            .common_dir;
        let found = (self.dir.join(repo_key(&common)), common);
        self.repos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(root.to_path_buf(), found.clone());
        Ok(found)
    }

    /// Creates a worktree on a new branch from a snapshot of `source`'s working tree; `None` outside git.
    pub async fn create(&self, source: &Path, label: &str) -> Result<Option<Worktree>, String> {
        let Some(repo) = git::repo(source).await else {
            return Ok(None);
        };
        let base = git::snapshot_repo(&repo).await?;
        let (source, common_dir) = (repo.top, repo.common_dir);
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
        };
        {
            let _guard = WORKTREE_LOCK.lock().await;
            let path = worktree.path.to_string_lossy().into_owned();
            let args = [
                "worktree",
                "add",
                "--quiet",
                "-b",
                &worktree.branch,
                &path,
                &worktree.base,
            ];
            git::run(&source, &args, &[]).await?;
        }
        share::share_ignored(&source, &worktree.path, &worktree.common_dir).await?;
        worktree.save(&repo_dir).await?;
        Ok(Some(worktree))
    }

    /// The worktree `id` of the repository containing `root`, if it still exists.
    pub async fn find(&self, root: &Path, id: &str) -> Option<Worktree> {
        let (repo_dir, _) = self.repo_dir(root).await.ok()?;
        let text = tokio::fs::read_to_string(repo_dir.join(format!("{id}.json")))
            .await
            .ok()?;
        let worktree: Worktree = serde_json::from_str(&text).ok()?;
        worktree.path.is_dir().then_some(worktree)
    }

    /// Every existing worktree of the repository containing `root`, oldest first.
    pub async fn list(&self, root: &Path) -> Vec<Worktree> {
        let Ok((repo_dir, _)) = self.repo_dir(root).await else {
            return Vec::new();
        };
        let Ok(mut entries) = tokio::fs::read_dir(&repo_dir).await else {
            return Vec::new();
        };
        let mut found = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(worktree) = read_worktree(&path).await
            {
                found.push(worktree);
            }
        }
        found.sort_by_key(|w| w.created);
        found
    }
}

async fn read_worktree(path: &Path) -> Option<Worktree> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    let worktree: Worktree = serde_json::from_str(&text).ok()?;
    worktree.path.is_dir().then_some(worktree)
}

impl Worktree {
    async fn save(&self, repo_dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        tokio::fs::write(repo_dir.join(format!("{}.json", self.id)), text)
            .await
            .map_err(|e| format!("could not save worktree metadata: {e}"))
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

    /// A stat summary and patch of everything the worktree changed, clipped for display.
    pub async fn diff(&self) -> Result<String, String> {
        let head = self.commit().await?;
        let stat = git::text(&self.path, &["diff", "--stat", &self.base, &head]).await?;
        let patch = git::run(&self.path, &["diff", &self.base, &head], &[]).await?;
        let patch = String::from_utf8_lossy(&patch);
        let mut out = format!("{stat}\n\n");
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

    /// Deletes the worktree, its branch, and its metadata.
    pub async fn remove(&self) -> Result<(), String> {
        let path = self.path.to_string_lossy().into_owned();
        let git_dir = format!("--git-dir={}", self.common_dir.display());
        {
            let _guard = WORKTREE_LOCK.lock().await;
            git::run(
                &self.common_dir,
                &[&git_dir, "worktree", "remove", "--force", &path],
                &[],
            )
            .await?;
        }
        git::run(
            &self.common_dir,
            &[&git_dir, "branch", "-D", "--quiet", &self.branch],
            &[],
        )
        .await?;
        if let Some(repo_dir) = self.path.parent() {
            let _ = tokio::fs::remove_file(repo_dir.join(format!("{}.json", self.id))).await;
        }
        Ok(())
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

        let wt = store.create(&root, "subagent perf audit").await.unwrap().unwrap();

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
        assert_eq!(store.create(dir.path(), "x").await.unwrap(), None);
    }
}
