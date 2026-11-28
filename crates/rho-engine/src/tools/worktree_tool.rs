use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
use crate::worktree::{REVIEW_FINDINGS_SHOWN, Worktree, WorktreeStore};
use rho_harness_core::args::{WorktreeAction, WorktreeArgs};
use rho_harness_core::error::AppError;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The main agent's view of worktrees: review/merge/discard agent worktrees, or create & enter a session worktree.
pub(crate) struct WorktreeTool {
    root: crate::tools::WorkspaceDir,
    store: WorktreeStore,
    session_base: Option<PathBuf>,
    clean_source: bool,
    relocate_slot: Option<std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>>,
}

impl WorktreeTool {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, store: WorktreeStore) -> Self {
        Self {
            root: crate::tools::WorkspaceDir::new(root),
            store,
            session_base: None,
            clean_source: false,
            relocate_slot: None,
        }
    }

    #[must_use]
    pub(crate) fn with_workspace_dir(mut self, root: crate::tools::WorkspaceDir) -> Self {
        self.root = root;
        self
    }

    #[must_use]
    pub(crate) fn with_session_worktree(
        mut self,
        base: PathBuf,
        clean_source: bool,
        relocate_slot: Option<std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>>,
    ) -> Self {
        self.session_base = Some(base);
        self.clean_source = clean_source;
        self.relocate_slot = relocate_slot;
        self
    }
    async fn find_in(&self, root: &std::path::Path, id: Option<&str>) -> Result<Worktree, String> {
        let id = id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or("this action needs an `id`")?;
        self.store
            .find(root, id)
            .await
            .ok_or_else(|| format!("no worktree '{id}'; it may already be merged or discarded (see action list)"))
    }

    async fn find(&self, id: Option<&str>) -> Result<Worktree, String> {
        let root = self.root.get();
        self.find_in(&root, id).await
    }

    async fn list(&self) -> Result<String, String> {
        let root = self.root.get();
        let worktrees = self.store.list(&root).await;
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
                let root = self.root.get();
                let worktree = self.find_in(&root, id).await?;
                let head = worktree.commit().await?;
                let review = worktree.review_head(&head).await?;
                let outcome = worktree.merge_head_into(&head, &root).await?;
                Ok(format!(
                    "{} Worktree `{}` was removed. Review the merged files and re-run your checks.\n\n{}",
                    outcome.summary(),
                    worktree.id,
                    review.render(REVIEW_FINDINGS_SHOWN)
                ))
            }
            WorktreeAction::Discard => {
                let worktree = self.find(id).await?;
                worktree.remove().await?;
                Ok(format!("Discarded worktree `{}` and its changes.", worktree.id))
            }
            WorktreeAction::Create => {
                let branch = args
                    .branch
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .map_or_else(crate::worktree::default_session_branch, str::to_string);
                let base = self
                    .session_base
                    .clone()
                    .unwrap_or_else(|| rho_harness_core::config::default_config_dir().join("wt"));
                let root = self.root.get();
                let created =
                    crate::worktree::create_session_worktree(&root, &base, &branch, self.clean_source).await?;
                if let Some(slot) = &self.relocate_slot {
                    *crate::locking::lock(slot) = Some(created.path.clone());
                }
                self.root.set(created.path.clone());
                Ok(created.summary())
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
        let keep = store.create(&root, "subagent fix", None).await.unwrap().unwrap();
        write(&keep.path, "a.rs", "fn a() { fixed() }\n");
        let drop = store.create(&root, "subagent spike", None).await.unwrap().unwrap();
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

    #[tokio::test]
    async fn create_immediately_relocates_shared_workspace_tools_and_isolates_subagents() {
        let (dir, root) = create(&[("a.rs", "fn a() {}\n")]);
        let store = WorktreeStore::new(dir.path().join("store"));
        let session_base = dir.path().join("session-wt");
        let relocate_slot = std::sync::Arc::new(std::sync::Mutex::new(None));

        let parent_ws = crate::tools::WorkspaceDir::new(&root);
        let parent_wt = WorktreeTool::new(&root, store)
            .with_workspace_dir(parent_ws.clone())
            .with_session_worktree(session_base, false, Some(relocate_slot.clone()));
        let parent_write = crate::tools::write::WriteTool::new(&root).with_workspace_dir(parent_ws.clone());
        let parent_read = crate::tools::read::ReadTool::new(&root).with_workspace_dir(parent_ws.clone());

        // Independent subagent tool keeps its own WorkspaceDir.
        let subagent_read = crate::tools::read::ReadTool::new(&root);

        let res = call(&parent_wt, json!({ "action": "create", "branch": "feat/immediate" })).await;
        assert!(!res.is_error, "{}", res.content);

        let new_path = relocate_slot.lock().unwrap().clone().expect("relocate slot populated");
        assert_ne!(new_path, root);
        assert_eq!(parent_ws.get(), new_path);

        // Next tool call in the same turn writes into the new worktree, not the original repo.
        let write_res = parent_write
            .execute(crate::tools::WriteArgs {
                path: "new_file.txt".to_string(),
                content: "hello worktree\n".to_string(),
            })
            .await
            .unwrap();
        assert!(!write_res.is_error, "{}", write_res.content);
        assert!(new_path.join("new_file.txt").exists());
        assert!(!root.join("new_file.txt").exists());

        let read_res = parent_read
            .execute(crate::tools::ReadArgs {
                path: "new_file.txt".to_string(),
                offset: None,
                limit: None,
                symbol: None,
            })
            .await
            .unwrap();
        assert!(!read_res.is_error, "{}", read_res.content);
        assert!(read_res.content.contains("hello worktree"));

        // Subagent tool still sees the original root, unaffected by parent relocation.
        assert_eq!(subagent_read.base_dir.get(), root);
        let sub_read_res = subagent_read
            .execute(crate::tools::ReadArgs {
                path: "new_file.txt".to_string(),
                offset: None,
                limit: None,
                symbol: None,
            })
            .await
            .unwrap();
        assert!(sub_read_res.is_error);
    }
}
