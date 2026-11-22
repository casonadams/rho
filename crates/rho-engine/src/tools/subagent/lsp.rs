//! The `lsp` tool as subagents get it: rooted in their worktree, and without `rename` unless the
//! subagent can change files.

use crate::lsp::LspManager;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::{ToolResult, parse_tool_args};
use rho_harness_core::args::{LspAction, LspArgs};
use rho_harness_core::error::AppError;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

/// Opens the language-server manager for a subagent's worktree.
pub type WorktreeLsp = Arc<dyn Fn(&Path) -> Arc<LspManager> + Send + Sync>;

/// `tools` with `lsp` limited to queries when neither `write` nor `edit` is among them.
pub(crate) fn restrict_lsp(tools: Vec<Arc<dyn EngineTool>>) -> Vec<Arc<dyn EngineTool>> {
    if tools.iter().any(|tool| matches!(tool.name(), "write" | "edit")) {
        return tools;
    }
    tools
        .into_iter()
        .map(|tool| match tool.name() {
            "lsp" => Arc::new(ReadOnlyLsp::new(tool)) as Arc<dyn EngineTool>,
            _ => tool,
        })
        .collect()
}

struct ReadOnlyLsp {
    inner: Arc<dyn EngineTool>,
    description: String,
}

impl ReadOnlyLsp {
    fn new(inner: Arc<dyn EngineTool>) -> Self {
        let description = format!(
            "{}\n- This agent cannot rename, format, or apply code actions: those are unavailable.",
            inner.description()
        );
        Self { inner, description }
    }
}

fn without_writes(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get_mut("properties") {
                properties.remove("new_name");
                properties.remove("apply");
            }
            for (key, child) in map.iter_mut() {
                if matches!(key.as_str(), "enum" | "oneOf" | "anyOf")
                    && let Value::Array(items) = child
                {
                    items.retain(|item| !is_writing_action(item));
                }
                without_writes(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(without_writes),
        _ => {}
    }
}

fn is_writing_action(item: &Value) -> bool {
    let name = item.as_str().or_else(|| item.get("const").and_then(Value::as_str));
    matches!(name, Some("rename" | "format"))
}

fn writes_files(args: &LspArgs) -> bool {
    match args.action {
        LspAction::Rename | LspAction::Format => true,
        LspAction::CodeActions => args.apply == Some(true),
        _ => false,
    }
}

#[async_trait::async_trait]
impl EngineTool for ReadOnlyLsp {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        let mut schema = self.inner.parameters();
        without_writes(&mut schema);
        schema
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        match parse_tool_args::<LspArgs>("lsp", args.clone(), || self.parameters()) {
            Ok(parsed) if writes_files(&parsed) => Ok(ToolResult::error(
                "lsp rename, format, and applying code actions are not available to this agent",
            )),
            Ok(_) => self.inner.execute(args).await,
            Err(invalid) => Ok(invalid),
        }
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        self.inner
            .bind_turn(ctx)
            .map(|bound| Arc::new(Self::new(bound)) as Arc<dyn EngineTool>)
    }

    fn concurrency_safe(&self, args: &Value) -> bool {
        self.inner.concurrency_safe(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::test_server::{FakeOptions, fake_manager};
    use crate::tools::builtin_tools::rerooted;
    use crate::tools::lsp::LspTool;
    use serde_json::json;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn helper() {}\n").unwrap();
        dir
    }

    fn named(name: &'static str) -> Arc<dyn EngineTool> {
        Arc::new(crate::tools::GenericTool::new(name, "desc", json!({}), |_| async {
            Ok(ToolResult::success("ok"))
        }))
    }

    fn lsp_in(dir: &Path) -> Arc<dyn EngineTool> {
        let (manager, _) = fake_manager(dir, FakeOptions::default());
        Arc::new(LspTool::new(dir, manager))
    }

    fn rename_call() -> Value {
        json!({"action": "rename", "path": "src/lib.rs", "line": 1, "symbol": "helper", "new_name": "assist"})
    }

    #[tokio::test]
    async fn read_only_lsp_hides_rename_from_the_schema_and_refuses_it() {
        let dir = project();
        let (manager, launcher) = fake_manager(dir.path(), FakeOptions::default());
        let full: Arc<dyn EngineTool> = Arc::new(LspTool::new(dir.path(), manager));
        let full_schema = full.parameters().to_string();
        assert!(full_schema.contains("rename") && full_schema.contains("new_name"));

        let tools = restrict_lsp(vec![named("read"), full]);
        let lsp = tools.iter().find(|t| t.name() == "lsp").unwrap();
        let schema = lsp.parameters().to_string();
        assert!(!schema.contains("rename") && !schema.contains("new_name"), "{schema}");
        assert!(
            !schema.contains("\"const\":\"format\"") && !schema.contains("\"apply\":"),
            "{schema}"
        );
        assert!(schema.contains("diagnostics") && schema.contains("definition") && schema.contains("code_actions"));

        let refused = lsp.execute(rename_call()).await.unwrap();
        assert!(refused.is_error && refused.content.contains("not available"));
        let format = json!({"action": "format", "path": "src/lib.rs"});
        let apply = json!({"action": "code_actions", "path": "src/lib.rs", "line": 1, "apply": true, "query": "1"});
        for call in [format, apply] {
            let refused = lsp.execute(call).await.unwrap();
            assert!(
                refused.is_error && refused.content.contains("not available"),
                "{}",
                refused.content
            );
        }
        assert!(launcher.received.lock().unwrap().is_empty());
        assert!(
            std::fs::read_to_string(dir.path().join("src/lib.rs"))
                .unwrap()
                .contains("helper")
        );
    }

    #[tokio::test]
    async fn read_only_lsp_still_answers_queries() {
        let dir = project();
        let lsp = restrict_lsp(vec![lsp_in(dir.path())]).remove(0);
        let status = lsp.execute(json!({"action": "status"})).await.unwrap();
        assert!(!status.is_error, "{}", status.content);
        assert!(lsp.description().contains("cannot rename"));
    }

    #[test]
    fn lsp_keeps_rename_when_the_subagent_can_change_files() {
        let dir = project();
        for writer in ["edit", "write"] {
            let tools = restrict_lsp(vec![named(writer), lsp_in(dir.path())]);
            let lsp = tools.iter().find(|t| t.name() == "lsp").unwrap();
            assert!(lsp.parameters().to_string().contains("rename"), "{writer}");
        }
    }

    #[tokio::test]
    async fn rerooted_tools_get_an_lsp_tool_for_the_worktree() {
        let parent = project();
        let worktree = project();
        let (manager, launcher) = fake_manager(worktree.path(), FakeOptions::default());
        let opened: WorktreeLsp = Arc::new(move |_| Arc::clone(&manager));
        let tools = vec![named("read"), lsp_in(parent.path())];

        let without = rerooted(&tools, worktree.path(), None);
        assert!(without.iter().all(|t| t.name() != "lsp"));

        let rooted = rerooted(&tools, worktree.path(), Some(&opened));
        let lsp = rooted.iter().find(|t| t.name() == "lsp").unwrap();
        assert!(!lsp.parameters().to_string().contains("rename"));
        let query = json!({"action": "symbols", "path": "src/lib.rs"});
        lsp.execute(query).await.unwrap();
        assert_eq!(launcher.launches.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn rerooted_write_tools_keep_the_full_lsp_tool() {
        let worktree = project();
        let (manager, _) = fake_manager(worktree.path(), FakeOptions::default());
        let opened: WorktreeLsp = Arc::new(move |_| Arc::clone(&manager));
        let tools = vec![named("edit"), lsp_in(worktree.path())];
        let rooted = rerooted(&tools, worktree.path(), Some(&opened));
        let lsp = rooted.iter().find(|t| t.name() == "lsp").unwrap();
        assert!(lsp.parameters().to_string().contains("rename"));
    }
}
