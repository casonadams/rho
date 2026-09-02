use crate::plugin::protocol::PluginEvent;
use rig::agent::hook::{CompletionCall, CompletionResponse, InvalidToolCallContext, ToolCall, ToolResultEvent};
use serde_json::{Value, json};

pub fn tool_call_event(event: ToolCall<'_>) -> Value {
    let args = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
    json!(PluginEvent::ToolCall {
        tool_name: event.tool_name.to_string(),
        args,
    })
}

pub fn tool_result_event(event: ToolResultEvent<'_>) -> Value {
    let args = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
    let output = event.presentation.render();
    json!(PluginEvent::ToolResult {
        tool_name: event.tool_name.to_string(),
        args,
        output,
        is_error: !event.raw_result.is_success(),
    })
}

pub fn invalid_tool_call_event(event: &InvalidToolCallContext) -> Value {
    let args = event
        .args
        .as_deref()
        .and_then(|a| serde_json::from_str::<Value>(a).ok())
        .unwrap_or(Value::Null);
    json!(PluginEvent::InvalidToolCall {
        tool_name: event.tool_name.clone(),
        args,
        available_tools: event.available_tools.clone(),
    })
}

pub fn completion_call_event(event: CompletionCall<'_>) -> Value {
    let prompt = serde_json::to_value(event.prompt).unwrap_or(Value::Null);
    let history = event
        .history
        .iter()
        .map(|msg| serde_json::to_value(msg).unwrap_or(Value::Null))
        .collect();
    json!(PluginEvent::CompletionCall {
        turn: event.turn,
        prompt,
        history,
    })
}

pub fn completion_response_event(event: CompletionResponse<'_>) -> Value {
    let prompt = serde_json::to_value(event.prompt).unwrap_or(Value::Null);
    let response = serde_json::to_value(event.content).unwrap_or(Value::Null);
    json!(PluginEvent::CompletionResponse { prompt, response })
}

pub fn text_delta_event(delta: &str) -> Value {
    json!(PluginEvent::TextDelta {
        delta: delta.to_string(),
    })
}

pub fn reasoning_delta_event(delta: &str) -> Value {
    json!(PluginEvent::ReasoningDelta {
        delta: delta.to_string(),
    })
}
