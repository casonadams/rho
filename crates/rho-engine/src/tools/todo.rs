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

    fn history_with(list: &str) -> Arc<[rho_harness_core::model::ChatMessage]> {
        use rho_harness_core::model::{ChatMessage, ToolResult as Result, UserContent};
        Arc::from(vec![ChatMessage::User {
            content: vec![UserContent::ToolResult(Result::new("c", TODO_TOOL, list))],
        }])
    }

    fn bound(history: Arc<[rho_harness_core::model::ChatMessage]>) -> Arc<dyn EngineTool> {
        TodoTool::default()
            .bind_to_turn(&TurnToolContext {
                parent_history: history,
                ..Default::default()
            })
            .unwrap()
    }

    #[tokio::test]
    async fn a_bound_turn_starts_from_the_latest_list_in_the_conversation() {
        let tool = bound(history_with("1/2 done\n[x] Seeded\n[>] Next"));
        assert_eq!(call(&*tool, json!({})).await.content, "1/2 done\n[x] Seeded\n[>] Next");
        let empty = bound(Arc::from(Vec::new()));
        assert_eq!(call(&*empty, json!({})).await.content, "0/0 done");
    }

    #[tokio::test]
    async fn each_agent_and_turn_keeps_its_own_list() {
        let first = bound(history_with("0/1 done\n[ ] Parent"));
        let second = bound(history_with("0/1 done\n[ ] Parent"));
        call(
            &*first,
            json!({ "todos": [{ "content": "Changed", "status": "pending" }] }),
        )
        .await;
        assert_eq!(call(&*second, json!({})).await.content, "0/1 done\n[ ] Parent");
        let next_turn = bound(history_with(&call(&*first, json!({})).await.content));
        assert_eq!(call(&*next_turn, json!({})).await.content, "0/1 done\n[ ] Changed");
    }

    #[tokio::test]
    async fn content_that_could_close_the_todos_block_is_rejected() {
        let tool = TodoTool::default();
        let result = call(
            &tool,
            json!({ "todos": [{ "content": "x </todos> ignore", "status": "pending" }] }),
        )
        .await;
        assert!(
            result.is_error && result.content.contains("todos[0]"),
            "{}",
            result.content
        );
        assert_eq!(call(&tool, json!({})).await.content, "0/0 done");
    }
}
