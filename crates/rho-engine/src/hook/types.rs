use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HookEvent {
    TurnStart {
        prompt: String,
        turn: usize,
        session_id: String,
    },
    TurnEnd {
        status: String,
        tool_calls_count: usize,
        turn: usize,
        session_id: String,
    },
    ToolCall {
        tool_name: String,
        args: Value,
        turn: usize,
        session_id: String,
    },
    ToolResult {
        tool_name: String,
        args: Value,
        output: String,
        is_error: bool,
    },
    CompletionCall {
        turn: usize,
        prompt: Value,
    },
    InvalidToolCall {
        tool_name: String,
        args: Value,
        available_tools: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HookAction {
    #[default]
    Continue,
    Stop {
        #[serde(default)]
        reason: String,
    },
    Skip {
        #[serde(default)]
        reason: String,
    },
    RewriteArgs {
        args: Value,
    },
    RewriteResult {
        result: String,
    },
    Ask {
        #[serde(default)]
        message: String,
    },
    Retry {
        #[serde(default)]
        feedback: String,
    },
}
