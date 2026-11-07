mod checkpoint;
mod compaction_tree;
mod memory;
mod prune;
mod storage;
mod tree;

use super::{SessionEventKind, SessionManager};
use crate::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, ToolResult, UserContent};

pub(crate) use crate::test_support::temp_dir;

fn make_tool_calls(ids: &[&str]) -> Vec<AssistantContent> {
    ids.iter()
        .map(|id| {
            AssistantContent::ToolCall(ToolCall::new(
                *id,
                ToolFunction::new("read", serde_json::json!({"path": id})),
            ))
        })
        .collect()
}

fn make_tool_results(ids: &[&str]) -> Vec<UserContent> {
    ids.iter()
        .map(|id| UserContent::ToolResult(ToolResult::new(*id, "read", "ok")))
        .collect()
}

pub(crate) fn complete_tool_turn(ids: &[&str]) -> Vec<ChatMessage> {
    vec![
        ChatMessage::user("read files"),
        ChatMessage::Assistant {
            id: None,
            content: make_tool_calls(ids),
        },
        ChatMessage::User {
            content: make_tool_results(ids),
        },
        ChatMessage::assistant("done"),
    ]
}
