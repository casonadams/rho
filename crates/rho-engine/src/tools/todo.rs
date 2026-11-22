use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::types::ToolResult;
use rho_harness_core::args::{TodoArgs, TodoItem};
use rho_harness_core::error::AppError;
use rho_harness_core::todo::{TODO_TOOL, latest_todos, render_todos, validate_todos};
use std::sync::{Arc, Mutex, PoisonError};

/// Each turn binds a copy holding the latest list in that agent's conversation, so every agent
/// keeps its own list and it survives resume and compaction.
#[derive(Default)]
pub struct TodoTool {
    todos: Mutex<Vec<TodoItem>>,
}

#[async_trait::async_trait]
impl TypedTool for TodoTool {
    type Args = TodoArgs;
    const NAME: &'static str = TODO_TOOL;

    async fn run(&self, args: TodoArgs) -> Result<ToolResult, AppError> {
        let mut todos = self.todos.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(next) = args.todos {
            if let Err(error) = validate_todos(&next) {
                return Ok(ToolResult::error(error));
            }
            *todos = next;
        }
        Ok(ToolResult::success(render_todos(&todos)))
    }

    fn bind_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(Self {
            todos: Mutex::new(latest_todos(&ctx.parent_history).unwrap_or_default()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn call(tool: &dyn EngineTool, args: serde_json::Value) -> ToolResult {
        tool.execute(args).await.unwrap()
    }

    #[tokio::test]
    async fn each_call_replaces_the_list_and_a_call_without_todos_shows_it() {
        let tool = TodoTool::default();

        let empty = call(&tool, json!({})).await;
        let set = call(
            &tool,
            json!({ "todos": [
                { "content": "Write the parser", "status": "completed" },
                { "content": "Test it", "status": "in_progress" },
                { "content": "Document it", "status": "pending" },
            ] }),
        )
        .await;
        let replaced = call(&tool, json!({ "todos": [{ "content": "Ship", "status": "pending" }] })).await;
        let viewed = call(&tool, json!({})).await;

        assert_eq!(empty.content, "0/0 done");
        assert_eq!(
            set.content,
            "1/3 done\n[x] Write the parser\n[>] Test it\n[ ] Document it"
        );
        assert_eq!(replaced.content, "0/1 done\n[ ] Ship");
        assert_eq!(viewed.content, replaced.content);
        assert!(!tool.concurrency_safe(&json!({})));
    }

    #[tokio::test]
    async fn two_in_progress_items_are_rejected_and_keep_the_previous_list() {
        let tool = TodoTool::default();
        call(
            &tool,
            json!({ "todos": [{ "content": "Keep", "status": "in_progress" }] }),
        )
        .await;

        let rejected = call(
            &tool,
            json!({ "todos": [
                { "content": "A", "status": "in_progress" },
                { "content": "B", "status": "in_progress" },
            ] }),
        )
        .await;

        assert!(rejected.is_error);
        assert!(
            rejected.content.contains("Only one todo may be in_progress"),
            "{}",
            rejected.content
        );
        assert_eq!(call(&tool, json!({})).await.content, "0/1 done\n[>] Keep");
    }
}
