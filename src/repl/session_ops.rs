//! Frontend-neutral session operations. Each function performs one operation and
//! returns its outcome as text or a domain result; line mode, live mode and RPC only render it.

use std::path::{Path, PathBuf};

use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::ForkTarget;
use crate::repl::fork_launch::fork_status_message;
use rho_harness_core::session::SessionManager;
use rho_harness_core::session::tree::SessionTree;

pub(crate) const COMPACTING: &str = "Compacting conversation context...";
pub(crate) const NOTHING_TO_RECAP: &str = "Nothing to recap yet.";

pub(crate) async fn fork(
    sessions_dir: &Path,
    engine: &AgentEngine,
    target: Option<ForkTarget>,
    node_id: Option<&str>,
) -> Result<String> {
    let session_id = engine
        .session_manager
        .fork_session(sessions_dir, node_id)
        .await?
        .session_id;
    Ok(fork_status_message(&session_id, target, &engine.base_dir))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceRestore {
    Unchanged,
    Restored,
    Missing(PathBuf),
    Failed { path: PathBuf, error: String },
}

pub(crate) fn restore_workspace(
    saved: Option<PathBuf>,
    change_dir: impl FnOnce(&Path) -> std::io::Result<()>,
) -> WorkspaceRestore {
    let Some(saved) = saved else {
        return WorkspaceRestore::Unchanged;
    };
    if !saved.is_dir() {
        return WorkspaceRestore::Missing(saved);
    }
    match change_dir(&saved) {
        Ok(()) => WorkspaceRestore::Restored,
        Err(err) => WorkspaceRestore::Failed {
            path: saved,
            error: err.to_string(),
        },
    }
}

pub(crate) struct Resumed {
    pub session_id: String,
    pub workspace: WorkspaceRestore,
    pub tree: SessionTree,
}

pub(crate) async fn resume(session: &mut ReplSession, engine: &mut AgentEngine, session_id: &str) -> Result<Resumed> {
    let saved = SessionManager::cwd_for_session_async(&session.config.sessions_dir, session_id).await?;
    let workspace = restore_workspace(saved, |dir| std::env::set_current_dir(dir));
    let next = session.new_engine(Some(session_id)).await?;
    let tree = next.session_manager.load_tree().await?;
    engine.switch_to(next);
    session.config = engine.config.clone();
    session.resume_id = Some(session_id.to_string());
    Ok(Resumed {
        session_id: session_id.to_string(),
        workspace,
        tree,
    })
}

pub(crate) fn resumed_message(resumed: &Resumed) -> String {
    let id = &resumed.session_id;
    let note = match &resumed.workspace {
        WorkspaceRestore::Unchanged | WorkspaceRestore::Restored => String::new(),
        WorkspaceRestore::Missing(path) => format!(" (previous workspace {} no longer exists)", path.display()),
        WorkspaceRestore::Failed { path, error } => {
            format!(" (could not restore workspace {}: {error})", path.display())
        }
    };
    format!("Resumed session {id}{note}")
}

pub(crate) fn resume_failed_message(session_id: &str, err: &impl std::fmt::Display) -> String {
    format!("Failed to resume session {session_id}: {err}")
}

pub(crate) async fn name_session(engine: &AgentEngine, name: &str) -> Result<String> {
    engine.session_manager.set_session_name(name).await?;
    Ok(format!("Session name: \"{name}\""))
}

/// A failed rewind is reported as text so the session keeps running.
pub(crate) async fn rewind(engine: &AgentEngine, turn: usize) -> String {
    rewind_message(turn, engine.rewind_to_turn(turn).await)
}

pub(crate) fn rewind_message(turn: usize, outcome: Result<usize>) -> String {
    match outcome {
        Ok(retained) => format!("Rewound context to Turn {turn} ({retained} messages retained)"),
        Err(err) => format!("Rewind failed: {err}"),
    }
}

/// A failed compaction is reported as text so the session keeps running.
pub(crate) async fn compact(engine: &AgentEngine, instructions: Option<&str>) -> String {
    match engine.compact_session(instructions).await {
        Ok(stats) => compaction_message(&stats),
        Err(err) => format!("Compaction failed: {err}"),
    }
}

fn compaction_message(stats: &rho_engine::engine::CompactionStats) -> String {
    use crate::ui::interactive::footer::format_tokens;
    let before = format_tokens(stats.tokens_before as u64);
    let after = format_tokens(stats.tokens_after as u64);
    let saved = format_tokens(stats.saved_tokens as u64);
    format!("Compacted context: {before} -> {after} tokens (saved {saved})")
}

pub(crate) enum RecapOutcome {
    Text(String),
    Status(String),
}

pub(crate) async fn recap(engine: &AgentEngine) -> RecapOutcome {
    let cancel = crate::engine::runner::CancellationSignal::default();
    match engine.generate_recap(cancel).await {
        Ok(Some(text)) => RecapOutcome::Text(text),
        Ok(None) => RecapOutcome::Status(NOTHING_TO_RECAP.to_string()),
        Err(err) => RecapOutcome::Status(recap_failed_message(&err)),
    }
}

pub(crate) fn recap_failed_message(err: &impl std::fmt::Display) -> String {
    format!("Recap failed: {err}")
}

pub(crate) async fn enter_worktree(engine: &mut AgentEngine, branch: Option<&str>) -> String {
    match engine.enter_session_worktree(branch).await {
        Ok(created) => created.summary(),
        Err(err) => format!("Worktree failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    #[test]
    fn restore_workspace_maps_the_saved_cwd_to_an_outcome() {
        let scratch = TestDir::new();
        let existing = scratch.to_path_buf();
        let missing = scratch.join("gone");
        let ok = |_: &Path| -> std::io::Result<()> { Ok(()) };
        let denied = |_: &Path| -> std::io::Result<()> { Err(std::io::Error::other("permission denied")) };

        assert_eq!(restore_workspace(None, ok), WorkspaceRestore::Unchanged);
        assert_eq!(
            restore_workspace(Some(existing.clone()), ok),
            WorkspaceRestore::Restored
        );
        assert_eq!(
            restore_workspace(Some(missing.clone()), ok),
            WorkspaceRestore::Missing(missing)
        );
        assert_eq!(
            restore_workspace(Some(existing.clone()), denied),
            WorkspaceRestore::Failed {
                path: existing,
                error: "permission denied".to_string(),
            }
        );
    }

    #[test]
    fn rewind_failure_is_reported_as_text() {
        let message = rewind_message(3, Err(crate::error::AppError::Session("boom".to_string())));
        assert_eq!(message, "Rewind failed: Session error: boom");
    }

    #[test]
    fn resumed_message_reports_each_workspace_outcome() {
        let resumed = |workspace| Resumed {
            session_id: "s1".to_string(),
            workspace,
            tree: SessionTree::default(),
        };
        let cases = [
            (WorkspaceRestore::Unchanged, "Resumed session s1"),
            (WorkspaceRestore::Restored, "Resumed session s1"),
            (
                WorkspaceRestore::Missing(PathBuf::from("/w")),
                "Resumed session s1 (previous workspace /w no longer exists)",
            ),
            (
                WorkspaceRestore::Failed {
                    path: PathBuf::from("/w"),
                    error: "denied".to_string(),
                },
                "Resumed session s1 (could not restore workspace /w: denied)",
            ),
        ];
        for (workspace, expected) in cases {
            assert_eq!(resumed_message(&resumed(workspace)), expected);
        }
    }
}
