mod checkpoint;
mod compaction_tree;
mod cwd_index;
mod durability;
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

/// The messages a run checkpoints before its final answer: a prompt and one tool round.
pub(crate) fn tool_round(prompt: &str, ids: &[&str]) -> Vec<ChatMessage> {
    vec![
        ChatMessage::user(prompt),
        ChatMessage::Assistant {
            id: None,
            content: make_tool_calls(ids),
        },
        ChatMessage::User {
            content: make_tool_results(ids),
        },
    ]
}

pub(crate) const LEGACY_V2_ID: &str = "legacy-v2";

/// Two promoted turns as format version 2 wrote them: each `tree_node` repeats its checkpoint.
pub(crate) const LEGACY_V2_SESSION: &str = concat!(
    r#"{"record_type":"header","version":2,"session_id":"legacy-v2","created_at":"2026-01-02T03:04:05Z"}"#,
    "\n",
    r#"{"record_type":"run_checkpoint","sequence":1,"session_id":"legacy-v2","timestamp":"2026-01-02T03:04:06Z","messages":[{"role":"user","content":[{"type":"text","text":"first prompt"}]},{"role":"assistant","content":[{"type":"toolcall","id":"call-1","function":{"name":"read","arguments":{"path":"call-1"}}}]},{"role":"user","content":[{"type":"toolresult","call":"call-1","name":"read","content":[{"type":"text","text":"ok"}]}]}]}"#,
    "\n",
    r#"{"record_type":"tree_node","sequence":2,"session_id":"legacy-v2","node":{"id":"node-1","parent_id":null,"timestamp":"2026-01-02T03:04:07Z","kind":"assistant_turn","messages":[{"role":"user","content":[{"type":"text","text":"first prompt"}]},{"role":"assistant","content":[{"type":"toolcall","id":"call-1","function":{"name":"read","arguments":{"path":"call-1"}}}]},{"role":"user","content":[{"type":"toolresult","call":"call-1","name":"read","content":[{"type":"text","text":"ok"}]}]},{"role":"assistant","content":[{"type":"text","text":"first answer"}]}]}}"#,
    "\n",
    r#"{"record_type":"run_checkpoint","sequence":3,"session_id":"legacy-v2","timestamp":"2026-01-02T03:04:08Z","messages":[{"role":"user","content":[{"type":"text","text":"second prompt"}]},{"role":"assistant","content":[{"type":"toolcall","id":"call-2","function":{"name":"read","arguments":{"path":"call-2"}}}]},{"role":"user","content":[{"type":"toolresult","call":"call-2","name":"read","content":[{"type":"text","text":"ok"}]}]}]}"#,
    "\n",
    r#"{"record_type":"tree_node","sequence":4,"session_id":"legacy-v2","node":{"id":"node-2","parent_id":"node-1","timestamp":"2026-01-02T03:04:09Z","kind":"assistant_turn","messages":[{"role":"user","content":[{"type":"text","text":"second prompt"}]},{"role":"assistant","content":[{"type":"toolcall","id":"call-2","function":{"name":"read","arguments":{"path":"call-2"}}}]},{"role":"user","content":[{"type":"toolresult","call":"call-2","name":"read","content":[{"type":"text","text":"ok"}]}]},{"role":"assistant","content":[{"type":"text","text":"second answer"}]}]}}"#,
    "\n",
);

/// The messages of each node in `LEGACY_V2_SESSION`, root first.
pub(crate) fn legacy_v2_turns() -> [Vec<ChatMessage>; 2] {
    [("first", "call-1"), ("second", "call-2")].map(|(ordinal, call)| {
        let mut turn = tool_round(&format!("{ordinal} prompt"), &[call]);
        turn.push(ChatMessage::assistant(format!("{ordinal} answer")));
        turn
    })
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
