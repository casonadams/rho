use rig::message::{AssistantContent, Message, UserContent};

pub const MAX_TOOL_RESULT_CHARS: usize = 2000;

pub fn serialize_conversation(messages: &[Message]) -> String {
    let mut blocks = Vec::new();
    for msg in messages {
        match msg {
            Message::System { content } => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    blocks.push(format!("[System]: {trimmed}"));
                }
            }
            Message::User { content } => {
                for item in content {
                    match item {
                        UserContent::Text(text) => {
                            let trimmed = text.text.trim();
                            if !trimmed.is_empty() {
                                blocks.push(format!("[User]: {trimmed}"));
                            }
                        }
                        UserContent::ToolResult(result) => {
                            let text = result
                                .content
                                .iter()
                                .filter_map(|c| c.as_text())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let total_chars = text.chars().count();
                            let formatted = if total_chars > MAX_TOOL_RESULT_CHARS {
                                let truncated: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
                                let omitted = total_chars - MAX_TOOL_RESULT_CHARS;
                                format!("{truncated}\n[... truncated {omitted} characters ...]")
                            } else {
                                text
                            };
                            blocks.push(format!("[Tool result]: {formatted}"));
                        }
                        _ => {}
                    }
                }
            }
            Message::Assistant { content, .. } => {
                for item in content {
                    match item {
                        AssistantContent::Text(text) => {
                            let trimmed = text.text.trim();
                            if !trimmed.is_empty() {
                                blocks.push(format!("[Assistant]: {trimmed}"));
                            }
                        }
                        AssistantContent::ToolCall(call) => {
                            let name = &call.function.name;
                            let args = &call.function.arguments;
                            blocks.push(format!("[Assistant tool call]: {name}({args})"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    blocks.join("\n\n")
}
