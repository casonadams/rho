use super::round::{Round, RoundEnd, RoundInput};
use super::runner::SubagentRunner;
use super::workspace::WorkspaceChange;
use crate::worktree::{FileChange, Worktree, WorktreeLease, git};
use rho_harness_core::model::ChatMessage;
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
    let settled = worktree.settle().await?;
    Ok(WorktreeRecord {
        id: worktree.id.clone(),
        path: worktree.path.display().to_string(),
        branch: worktree.branch.clone(),
        head: settled.head,
        changes: settled.changes,
        removed: settled.removed,
    })
}

/// A round run in its worktree, which stays leased until `settle`: settling removes a worktree with no
/// changes, so the files the round touched are read first.
pub(crate) struct WorktreeRound {
    pub round: Round,
    lease: Option<WorktreeLease>,
}

/// A round whose worktree is settled and released.
pub(crate) struct SettledRound {
    pub round: Round,
    /// The worktree's record, or the error settling it; `None` when the round had no worktree.
    pub record: Option<Result<WorktreeRecord, String>>,
    /// The released worktree.
    pub worktree: Option<Worktree>,
}

impl WorktreeRound {
    /// Settles the worktree and releases its lease. A settle error is returned beside the round, which
    /// still stands.
    pub(crate) async fn settle(self) -> SettledRound {
        let record = match &self.lease {
            Some(lease) => Some(settle(lease.worktree()).await),
            None => None,
        };
        SettledRound {
            round: self.round,
            record,
            worktree: self.lease.map(WorktreeLease::release),
        }
    }
}

/// Runs `input` from `transcript` in the worktree `lease` holds, or in the runner's own workspace without
/// one, with the worktree's language servers stopped afterwards.
pub(crate) async fn run_in_worktree(
    runner: &SubagentRunner,
    lease: Option<WorktreeLease>,
    transcript: Vec<ChatMessage>,
    input: RoundInput<'_>,
) -> WorktreeRound {
    let runner = match &lease {
        Some(lease) => runner.clone().in_worktree(&lease.worktree().path),
        None => runner.clone(),
    };
    let lsp_guard = runner.worktree_lsp_guard();
    let round = runner.run_round(transcript, input).await;
    lsp_guard.close_unless_cancelled(round.end == RoundEnd::Cancelled).await;
    WorktreeRound { round, lease }
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
        "\n\nThese changes are in worktree `{id}` ({path}, branch {branch}), not in your working tree: {files}. Review them with worktree {{\"action\": \"diff\", \"id\": \"{id}\"}}. {standard} Merge with {{\"action\": \"merge\", \"id\": \"{id}\"}} (conflicts are left as markers for you to fix) or drop them with {{\"action\": \"discard\", \"id\": \"{id}\"}}. You are responsible for this worktree: unmerged worktrees are deleted 24 hours after they were created.",
        id = record.id,
        path = record.path,
        branch = record.branch,
        files = files.join(", "),
        standard = crate::worktree::MERGE_STANDARD
    )
}
