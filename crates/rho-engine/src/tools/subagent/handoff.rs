use super::workspace::WorkspaceChange;
use crate::worktree::{FileChange, Worktree, git};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const WORKTREE_ARTIFACT: &str = "worktree";

/// A task's worktree as of the end of its round: where its unmerged changes are, or that it had none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorktreeRecord {
    pub id: String,
    pub path: String,
    pub branch: String,
    /// The worktree's commit at the end of the round: what the agent last saw.
    pub head: String,
    pub changes: Vec<FileChange>,
    /// Removed because it held no changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
}

/// Commits the worktree after a round; one without changes is removed since there is nothing to merge.
pub(crate) async fn settle(worktree: &Worktree) -> Result<WorktreeRecord, String> {
    let (head, changes) = worktree.changes().await?;
    let removed = changes.is_empty();
    if removed {
        worktree.remove().await?;
    }
    Ok(WorktreeRecord {
        id: worktree.id.clone(),
        path: worktree.path.display().to_string(),
        branch: worktree.branch.clone(),
        head,
        changes,
        removed,
    })
}

/// Files that differ from `last_head`, the commit the agent last saw. `now` is the commit it starts from
/// when known (a fresh worktree's base); otherwise `root` is snapshotted.
pub(crate) async fn changed_since(root: &Path, last_head: &str, now: Option<&str>) -> Vec<WorkspaceChange> {
    let now = match now {
        Some(commit) => commit.to_string(),
        None => match git::snapshot(root).await {
            Ok(commit) => commit,
            Err(_) => return Vec::new(),
        },
    };
    git::changed_paths(root, last_head, &now)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(status, path)| WorkspaceChange {
            path,
            deleted: status == 'D',
            by: None,
        })
        .collect()
}

/// Tells a continued subagent its earlier worktree is gone and where it works now.
pub(crate) fn moved_note(previous: &WorktreeRecord, current: &Worktree) -> String {
    format!(
        "[Your previous worktree {} no longer exists (its changes were merged or discarded). You now work in {}, a fresh copy of the current project; paths from earlier messages that pointed into the old worktree are stale.]\n\n",
        previous.path,
        current.path.display()
    )
}

/// What the parent must do with the changes, appended to the subagent's result.
pub(crate) fn pending_note(record: &WorktreeRecord) -> String {
    if record.removed {
        return String::new();
    }
    let files: Vec<String> = record.changes.iter().map(FileChange::label).collect();
    format!(
        "\n\nThese changes are in worktree `{id}` ({path}, branch {branch}), not in your working tree: {files}. Review them with worktree {{\"action\": \"diff\", \"id\": \"{id}\"}}, then merge with {{\"action\": \"merge\", \"id\": \"{id}\"}} (conflicts are left as markers for you to fix) or drop them with {{\"action\": \"discard\", \"id\": \"{id}\"}}.",
        id = record.id,
        path = record.path,
        branch = record.branch,
        files = files.join(", ")
    )
}
