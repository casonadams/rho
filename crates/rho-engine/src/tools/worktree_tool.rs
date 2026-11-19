use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
use crate::worktree::{Worktree, WorktreeStore};
use rho_harness_core::args::{WorktreeAction, WorktreeArgs};
use rho_harness_core::error::AppError;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The main agent's view of agent worktrees: review, merge into its working tree, or discard.
pub struct WorktreeTool {
    root: PathBuf,
    store: WorktreeStore,
}

impl WorktreeTool {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, store: WorktreeStore) -> Self {
        Self {
            root: root.into(),
            store,
        }
    }

    async fn find(&self, id: Option<&str>) -> Result<Worktree, String> {
        let id = id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or("this action needs an `id`")?;
        self.store
            .find(&self.root, id)
            .await
            .ok_or_else(|| format!("no worktree '{id}'; it may already be merged or discarded (see action list)"))
    }

    async fn list(&self) -> Result<String, String> {
        let worktrees = self.store.list(&self.root).await;
        if worktrees.is_empty() {
            return Ok("No agent worktrees are waiting to be merged.".to_string());
        }
        let mut out = String::from("Agent worktrees, oldest first:\n");
        for worktree in &worktrees {
            let (_, changes) = worktree.changes().await?;
            let _ = writeln!(
                out,
                "- {} [{}], created {}",
                worktree.summary(&changes),
                worktree.label,
                worktree.created.format("%Y-%m-%d %H:%M UTC")
            );
        }
        Ok(out)
    }

    async fn run(&self, args: WorktreeArgs) -> Result<String, String> {
        let id = args.id.as_deref();
        match args.action {
            WorktreeAction::List => self.list().await,
            WorktreeAction::Diff => self.find(id).await?.diff().await,
            WorktreeAction::Merge => {
                let worktree = self.find(id).await?;
                let outcome = worktree.merge_into(&self.root).await?;
                Ok(format!(
                    "{} Worktree `{}` was removed. Review the merged files and re-run your checks.",
                    outcome.summary(),
                    worktree.id
                ))
            }
            WorktreeAction::Discard => {
                let worktree = self.find(id).await?;
                worktree.remove().await?;
                Ok(format!("Discarded worktree `{}` and its changes.", worktree.id))
            }
        }
    }
}

#[async_trait::async_trait]
impl EngineTool for WorktreeTool {
    fn name(&self) -> &str {
        "worktree"
    }

    fn description(&self) -> &str {
        crate::tools::ToolRegistry::builtin_description("worktree")
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<WorktreeArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: WorktreeArgs = match crate::tools::types::parse_tool_args("worktree", args, || self.parameters()) {
            Ok(a) => a,
            Err(e) => return Ok(e),
        };
        Ok(match self.run(args).await {
            Ok(text) => ToolResult::success(text),
            Err(error) => ToolResult::error(error),
        })
    }

    fn concurrency_safe(&self, args: &serde_json::Value) -> bool {
        matches!(args["action"].as_str(), Some("list" | "diff"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::git::test_repo::{create, write};
    use serde_json::json;

    async fn call(tool: &WorktreeTool, args: serde_json::Value) -> ToolResult {
        tool.execute(args).await.unwrap()
    }

    #[tokio::test]
    async fn the_parent_lists_reviews_merges_and_discards_agent_worktrees() {
        let (dir, root) = create(&[("a.rs", "fn a() {}\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let tool = WorktreeTool::new(&root, store.clone());
        let keep = store.create(&root, "subagent fix").await.unwrap().unwrap();
        write(&keep.path, "a.rs", "fn a() { fixed() }\n");
        let drop = store.create(&root, "subagent spike").await.unwrap().unwrap();
        write(&drop.path, "spike.rs", "x\n");

        let listed = call(&tool, json!({ "action": "list" })).await.content;
        let diff = call(&tool, json!({ "action": "diff", "id": keep.id })).await.content;
        let merged = call(&tool, json!({ "action": "merge", "id": keep.id })).await.content;
        let discarded = call(&tool, json!({ "action": "discard", "id": drop.id })).await.content;

        assert!(
            listed.contains(&format!("worktree `{}`", keep.id)) && listed.contains("[subagent spike]"),
            "{listed}"
        );
        assert!(diff.contains("+fn a() { fixed() }"), "{diff}");
        assert!(merged.starts_with("Merged 1 file(s): a.rs."), "{merged}");
        assert_eq!(
            std::fs::read_to_string(root.join("a.rs")).unwrap(),
            "fn a() { fixed() }\n"
        );
        assert!(discarded.starts_with("Discarded"), "{discarded}");
        assert!(!root.join("spike.rs").exists());
        assert!(
            call(&tool, json!({ "action": "list" }))
                .await
                .content
                .starts_with("No agent worktrees")
        );
        let gone = call(&tool, json!({ "action": "merge", "id": keep.id })).await;
        assert!(
            gone.is_error && gone.content.contains("already be merged"),
            "{}",
            gone.content
        );
    }
}
