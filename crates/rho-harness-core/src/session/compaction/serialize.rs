use rig::message::{AssistantContent, Message, UserContent};

pub const MAX_TOOL_RESULT_CHARS: usize = 2000;

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

fn push_user_content_block(item: &UserContent, blocks: &mut Vec<String>) {
    match item {
        UserContent::Text(text) => {
            let trimmed = text.text.trim();
            if !trimmed.is_empty() {
                blocks.push(format!("[User]: {trimmed}"));
            }
        }
        UserContent::ToolResult(result) => {
            let formatted = serialize_tool_result_content(result);
            blocks.push(format!("[Tool result]: {formatted}"));
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
            blocks.push(format!(
                "[Assistant tool call]: {}({})",
                call.function.name, call.function.arguments
            ));
        }
        _ => {}
    }
}

fn serialize_message(msg: &Message, blocks: &mut Vec<String>) {
    match msg {
        Message::System { content } => {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                blocks.push(format!("[System]: {trimmed}"));
            }
        }
        Message::User { content } => {
            for item in content {
                push_user_content_block(item, blocks);
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
    let mut blocks = Vec::new();
    for msg in messages {
        serialize_message(msg, &mut blocks);
    }
    blocks.join("\n\n")
}
