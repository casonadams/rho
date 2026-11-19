use super::{WORKTREE_LOCK, Worktree, WorktreeStore, git, meta_files, read_meta, repo_key};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// Unmerged agent work is deleted this long after its worktree was created.
pub const EXPIRE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// Leftovers younger than this may belong to a worktree another rho process is still creating.
const ORPHAN_GRACE: Duration = Duration::from_secs(60 * 60);
const BRANCH_PREFIX: &str = "refs/heads/rho/wt/";

/// Repositories already swept by this process.
static SWEPT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

#[derive(Debug, Default)]
pub struct SweepReport {
    /// Worktrees deleted because they were older than `EXPIRE_AFTER`.
    pub expired: Vec<Worktree>,
    /// Worktrees with unmerged work that were kept.
    pub pending: Vec<Worktree>,
    /// Leftover branches, checkouts, and metadata from interrupted runs that were removed.
    pub leftovers: usize,
}

struct Policy {
    now: DateTime<Utc>,
    expire_after: Duration,
    orphan_grace: Duration,
}

fn older_than(time: DateTime<Utc>, age: Duration, now: DateTime<Utc>) -> bool {
    chrono::Duration::from_std(age).is_ok_and(|age| now - time > age)
}

async fn branch_exists(common_dir: &Path, branch: &str) -> bool {
    let git_dir = format!("--git-dir={}", common_dir.display());
    let name = format!("refs/heads/{branch}");
    git::run(common_dir, &[&git_dir, "rev-parse", "--verify", "--quiet", &name], &[])
        .await
        .is_ok()
}

/// `(worktree id, commit time)` of every `rho/wt/*` branch.
async fn rho_branches(common_dir: &Path) -> Vec<(String, DateTime<Utc>)> {
    let git_dir = format!("--git-dir={}", common_dir.display());
    let args = [
        &git_dir,
        "for-each-ref",
        "--format=%(refname) %(committerdate:unix)",
        BRANCH_PREFIX,
    ];
    let Ok(out) = git::text(common_dir, &args).await else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|line| {
            let (name, time) = line.rsplit_once(' ')?;
            let id = name.strip_prefix(BRANCH_PREFIX)?.to_string();
            let time = DateTime::from_timestamp(time.parse().ok()?, 0)?;
            Some((id, time))
        })
        .collect()
}

async fn remove_checkout(common_dir: &Path, path: &Path) {
    let git_dir = format!("--git-dir={}", common_dir.display());
    let target = path.to_string_lossy().into_owned();
    let _guard = WORKTREE_LOCK.lock().await;
    if git::run(common_dir, &[&git_dir, "worktree", "remove", "--force", &target], &[])
        .await
        .is_err()
    {
        let _ = tokio::fs::remove_dir_all(path).await;
    }
}

impl WorktreeStore {
    /// Sweeps the repository at `common_dir` once per process; `None` if already swept.
    pub(crate) async fn sweep_once(&self, common_dir: &Path) -> Option<SweepReport> {
        let first = SWEPT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashSet::new)
            .insert(common_dir.to_path_buf());
        if !first {
            return None;
        }
        let policy = Policy {
            now: Utc::now(),
            expire_after: EXPIRE_AFTER,
            orphan_grace: ORPHAN_GRACE,
        };
        Some(self.sweep_with(common_dir, &policy).await)
    }

    async fn sweep_with(&self, common_dir: &Path, policy: &Policy) -> SweepReport {
        let mut report = SweepReport::default();
        let git_dir = format!("--git-dir={}", common_dir.display());
        {
            let _guard = WORKTREE_LOCK.lock().await;
            let _ = git::run(common_dir, &[&git_dir, "worktree", "prune"], &[]).await;
        }
        let mut known = BTreeSet::new();
        for meta in meta_files(common_dir).await {
            self.sweep_meta(common_dir, &meta, policy, &mut report, &mut known)
                .await;
        }
        let checkouts = self.dir.join(repo_key(common_dir));
        for (id, time) in rho_branches(common_dir).await {
            if known.contains(&id) || !older_than(time, policy.orphan_grace, policy.now) {
                continue;
            }
            remove_checkout(common_dir, &checkouts.join(&id)).await;
            let branch = format!("{BRANCH_PREFIX}{id}");
            if git::run(common_dir, &[&git_dir, "update-ref", "-d", &branch], &[])
                .await
                .is_ok()
            {
                report.leftovers += 1;
            }
            known.insert(id);
        }
        report.leftovers += sweep_checkouts(common_dir, &checkouts, &known, policy).await;
        report
    }

    async fn sweep_meta(
        &self,
        common_dir: &Path,
        meta: &Path,
        policy: &Policy,
        report: &mut SweepReport,
        known: &mut BTreeSet<String>,
    ) {
        let Some(worktree) = read_meta(meta).await else {
            let _ = tokio::fs::remove_file(meta).await;
            report.leftovers += 1;
            return;
        };
        known.insert(worktree.id.clone());
        if older_than(worktree.created, policy.expire_after, policy.now) {
            if worktree.remove().await.is_ok() {
                report.expired.push(worktree);
            }
        } else if !branch_exists(common_dir, &worktree.branch).await {
            if worktree.remove().await.is_ok() {
                report.leftovers += 1;
            }
        } else {
            report.pending.push(worktree);
        }
    }
}

/// Checkout directories that no worktree or branch accounts for.
async fn sweep_checkouts(common_dir: &Path, checkouts: &Path, known: &BTreeSet<String>, policy: &Policy) -> usize {
    let Ok(mut entries) = tokio::fs::read_dir(checkouts).await else {
        return 0;
    };
    let mut removed = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = entry.metadata().await.ok().and_then(|m| m.modified().ok());
        let stale = modified.is_some_and(|m| older_than(m.into(), policy.orphan_grace, policy.now));
        if !known.contains(&name) && stale {
            remove_checkout(common_dir, &entry.path()).await;
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::super::git::test_repo::{create, git, write};
    use super::*;

    #[tokio::test]
    async fn sweeping_expires_old_work_and_removes_leftovers_but_keeps_recent_work() {
        let (dir, root) = create(&[("a.rs", "base\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let fresh = store.create(&root, "fresh", Some("s1")).await.unwrap().unwrap();
        let mut old = store.create(&root, "old", Some("s0")).await.unwrap().unwrap();
        old.created = Utc::now() - chrono::Duration::hours(25);
        old.save().await.unwrap();
        let no_branch = store.create(&root, "no branch", None).await.unwrap().unwrap();
        git(
            &root,
            &["worktree", "remove", "--force", &no_branch.path.to_string_lossy()],
        );
        git(&root, &["branch", "-D", &no_branch.branch]);
        git(&root, &["branch", "rho/wt/crashed-1234"]);
        write(&store.dir.join(repo_key(&fresh.common_dir)), "stray/file", "x");
        let common_dir = fresh.common_dir.clone();
        let policy = Policy {
            now: Utc::now(),
            expire_after: EXPIRE_AFTER,
            orphan_grace: Duration::ZERO,
        };

        let report = store.sweep_with(&common_dir, &policy).await;

        assert_eq!(
            report.expired.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            [old.id.as_str()]
        );
        assert_eq!(
            report.pending.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            [fresh.id.as_str()]
        );
        assert_eq!(
            report.leftovers, 3,
            "missing-branch metadata, crashed branch, stray checkout"
        );
        assert!(!old.path.exists() && fresh.path.exists());
        let branches = git(&root, &["branch", "--list", "rho/wt/*"]);
        assert_eq!(branches.trim().trim_start_matches(['*', '+', ' ']), fresh.branch);
        assert!(!store.dir.join(repo_key(&common_dir)).join("stray").exists());
    }

    #[tokio::test]
    async fn a_cleared_checkout_is_recreated_from_its_branch() {
        let (dir, root) = create(&[("a.rs", "base\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let worktree = store.create(&root, "swept", Some("s1")).await.unwrap().unwrap();
        write(&worktree.path, "a.rs", "agent\n");
        worktree.commit().await.unwrap();
        std::fs::remove_dir_all(&worktree.path).unwrap();

        let found = store.find(&root, &worktree.id).await.unwrap();

        assert_eq!(std::fs::read_to_string(found.path.join("a.rs")).unwrap(), "agent\n");
        assert_eq!(store.owned(&root, "s1").await, [found]);
        assert!(store.owned(&root, "other").await.is_empty());
    }

    #[tokio::test]
    async fn a_worktree_that_cannot_be_finished_is_rolled_back() {
        let (dir, root) = create(&[("a.rs", "base\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let common_dir = PathBuf::from(git(&root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]));
        std::fs::create_dir_all(common_dir.join("rho")).unwrap();
        std::fs::write(common_dir.join("rho").join("worktrees"), "not a directory").unwrap();

        let error = store.create(&root, "broken", None).await.unwrap_err();

        assert!(
            error.contains("worktree metadata") || error.contains("File exists"),
            "{error}"
        );
        assert!(git(&root, &["branch", "--list", "rho/wt/*"]).is_empty());
        assert_eq!(git(&root, &["worktree", "list"]).lines().count(), 1);
    }
}
