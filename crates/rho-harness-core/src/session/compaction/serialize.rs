use rig::message::{AssistantContent, Message, UserContent};

pub const MAX_TOOL_RESULT_CHARS: usize = 2000;
pub const MAX_FULL_TOOL_RESULTS: usize = 3;
pub const MAX_ARG_STRING_CHARS: usize = 200;

fn format_truncated_tool_text(text: String) -> String {
    let total_chars = text.chars().count();
    if total_chars > MAX_TOOL_RESULT_CHARS {
        let truncated: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
        let omitted = total_chars - MAX_TOOL_RESULT_CHARS;
        format!("{truncated}\n[... truncated {omitted} characters ...]")
    } else {
        text
    }
}

fn serialize_tool_result_content(result: &rig::message::ToolResult) -> String {
    let mut total_chars = 0;
    let mut text = String::new();
    for t in result.content.iter().filter_map(|c| c.as_text()) {
        if !text.is_empty() {
            text.push('\n');
            total_chars += 1;
        }
        text.push_str(t);
        total_chars += t.chars().count();
        if total_chars > MAX_TOOL_RESULT_CHARS {
            break;
        }
    }
    format_truncated_tool_text(text)
}

fn truncate_arg_string(s: &str) -> String {
    let char_count = s.chars().count();
    if char_count > MAX_ARG_STRING_CHARS {
        let line_count = s.lines().count();
        let line_str = if line_count == 1 {
            "1 line".to_string()
        } else {
            format!("{line_count} lines")
        };
        format!("[truncated: {line_str}, {char_count} chars]")
    } else {
        s.to_string()
    }
}

fn sanitize_value_strings(val: &mut serde_json::Value, keys: &[&str]) {
    if let Some(obj) = val.as_object_mut() {
        for &key in keys {
            if let Some(field) = obj.get_mut(key)
                && let Some(s) = field.as_str()
                && s.chars().count() > MAX_ARG_STRING_CHARS
            {
                *field = serde_json::Value::String(truncate_arg_string(s));
            }
        }
        if let Some(edits) = obj.get_mut("edits").and_then(|v| v.as_array_mut()) {
            for edit in edits {
                sanitize_value_strings(edit, keys);
            }
        }
    }
}

fn sanitize_tool_arguments(tool_name: &str, arguments: &serde_json::Value) -> serde_json::Value {
    let mut val = match arguments {
        serde_json::Value::String(s) => match serde_json::from_str::<serde_json::Value>(s) {
            Ok(parsed) => parsed,
            Err(_) => return arguments.clone(),
        },
        other => other.clone(),
    };

    match tool_name {
        "write" => {
            sanitize_value_strings(&mut val, &["content"]);
            val
        }
        "edit" => {
            sanitize_value_strings(&mut val, &["oldText", "old_text", "newText", "new_text"]);
            val
        }
        _ => arguments.clone(),
    }
}

fn push_user_content_block(
    item: &UserContent,
    blocks: &mut Vec<String>,
    tool_result_idx: &mut usize,
    keep_full_from_idx: usize,
) {
    match item {
        UserContent::Text(text) => {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                blocks.push(format!("[User]: {trimmed}"));
            }
        }
        UserContent::ToolResult(result) => {
            let current_idx = *tool_result_idx;
            *tool_result_idx += 1;
            if current_idx < keep_full_from_idx {
                let name = result.name.trim();
                let stub = if name.is_empty() {
                    "[tool result]".to_string()
                } else {
                    format!("[tool result: {name}]")
                };
                blocks.push(stub);
            } else {
                let formatted = serialize_tool_result_content(result);
                blocks.push(format!("[Tool result]: {formatted}"));
            }
        }
        _ => {}
    }
}

fn push_assistant_content_block(item: &AssistantContent, blocks: &mut Vec<String>) {
    match item {
        AssistantContent::Text(text) => {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                blocks.push(format!("[Assistant]: {trimmed}"));
            }
        }
        AssistantContent::ToolCall(call) => {
            let sanitized = sanitize_tool_arguments(&call.function.name, &call.function.arguments);
            blocks.push(format!("[Assistant tool call]: {}({})", call.function.name, sanitized));
        }
        _ => {}
    }
}

fn serialize_message(msg: &Message, blocks: &mut Vec<String>, tool_result_idx: &mut usize, keep_full_from_idx: usize) {
    match msg {
        Message::System { content } => {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                blocks.push(format!("[System]: {trimmed}"));
            }
        }
        Message::User { content } => {
            for item in content {
                push_user_content_block(item, blocks, tool_result_idx, keep_full_from_idx);
            }
        }
        Message::Assistant { content, .. } => {
            for item in content {
                push_assistant_content_block(item, blocks);
            }
        }
    }
}

pub fn serialize_conversation(messages: &[Message]) -> String {
    let total_tool_results: usize = messages
        .iter()
        .map(|msg| match msg {
            Message::User { content } => content
                .iter()
                .filter(|item| matches!(item, UserContent::ToolResult(_)))
                .count(),
            _ => 0,
        })
        .sum();

    let keep_full_from_idx = total_tool_results.saturating_sub(MAX_FULL_TOOL_RESULTS);
    let mut tool_result_idx = 0;
    let mut blocks = Vec::new();
    for msg in messages {
        serialize_message(msg, &mut blocks, &mut tool_result_idx, keep_full_from_idx);
    }
    blocks.join("\n\n")
}
