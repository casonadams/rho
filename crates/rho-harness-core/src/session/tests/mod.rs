mod checkpoint;
mod compaction_tree;
mod memory;
mod prune;
mod storage;
mod tree;

use super::{SessionEventKind, SessionManager};
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use std::path::PathBuf;

pub(crate) fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("session_test_{}", uuid::Uuid::new_v4()))
}

fn make_tool_calls(ids: &[&str]) -> Vec<AssistantContent> {
    ids.iter()
        .map(|id| {
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new(*id).unwrap(),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": id})),
            ))
        })
        .collect()
}

fn make_tool_results(ids: &[&str]) -> Vec<UserContent> {
    ids.iter()
        .map(|id| {
            UserContent::ToolResult(ToolResult {
                call: ToolCallId::new(*id).unwrap(),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::text("ok")],
            })
        })
        .collect()
}

pub(crate) fn complete_tool_turn(ids: &[&str]) -> Vec<Message> {
    vec![
        Message::user("read files"),
        Message::Assistant {
            id: None,
            content: make_tool_calls(ids),
        },
        Message::User {
            content: make_tool_results(ids),
        },
        Message::assistant("done"),
    ]
}
