use super::Worktree;
use crate::locking::lock;
use std::sync::Mutex;
use tokio::task::JoinHandle;

/// Background settles started by dropped leases, so shutdown and tests can wait for them.
static ABANDONED: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// A worktree in use by a running agent. Releasing it hands the worktree on; dropping it unreleased
/// (the round was aborted or panicked) commits its work and removes it if it changed nothing.
pub struct WorktreeLease {
    worktree: Option<Worktree>,
}

impl WorktreeLease {
    #[must_use]
    pub fn new(worktree: Worktree) -> Self {
        Self {
            worktree: Some(worktree),
        }
    }

    #[must_use]
    pub fn worktree(&self) -> &Worktree {
        self.worktree
            .as_ref()
            .expect("a lease holds its worktree until released or dropped")
    }

    #[must_use]
    pub fn release(mut self) -> Worktree {
        self.worktree
            .take()
            .expect("a lease holds its worktree until released or dropped")
    }
}

impl Drop for WorktreeLease {
    fn drop(&mut self) {
        let Some(worktree) = self.worktree.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let task = runtime.spawn(async move {
            let _ = worktree.settle().await;
        });
        lock(&ABANDONED).push(task);
    }
}

/// Waits for worktrees abandoned by aborted rounds to be committed or removed.
pub async fn settle_abandoned() {
    let tasks: Vec<JoinHandle<()>> = std::mem::take(&mut *lock(&ABANDONED));
    for task in tasks {
        let _ = task.await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::WorktreeStore;
    use super::super::git::test_repo::{create, git, write};
    use super::*;

    #[tokio::test]
    async fn an_abandoned_lease_keeps_committed_work_and_removes_empty_worktrees() {
        let (dir, root) = create(&[("a.rs", "base\n")]);
        let store = WorktreeStore::new(dir.join("store"));
        let busy = store.create(&root, "busy", None).await.unwrap().unwrap();
        let idle = store.create(&root, "idle", None).await.unwrap().unwrap();
        write(&busy.path, "a.rs", "half done\n");

        drop(WorktreeLease::new(busy.clone()));
        drop(WorktreeLease::new(idle.clone()));
        settle_abandoned().await;

        let kept = store.list(&root).await;
        assert_eq!(
            kept.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            [busy.id.as_str()]
        );
        assert_eq!(git(&root, &["show", &format!("{}:a.rs", busy.branch)]), "half done");
        assert!(!idle.path.exists());
    }

    #[tokio::test]
    async fn a_released_lease_leaves_the_worktree_alone() {
        let (dir, root) = create(&[("a.rs", "base\n")]);
        let store = WorktreeStore::new(dir.join("store"));
        let worktree = store.create(&root, "kept", None).await.unwrap().unwrap();

        let released = WorktreeLease::new(worktree.clone()).release();
        settle_abandoned().await;

        assert_eq!(released, worktree);
        assert!(worktree.path.exists());
    }
}
