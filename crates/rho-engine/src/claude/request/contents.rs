//! Content and message conversions for Claude Messages API requests.

use crate::provider::adapter::ModelCompletionRequest;
use rho_harness_core::model::{AssistantContent, ChatMessage, ImageContent, Reasoning, ReasoningContent, UserContent};
use serde_json::{Value, json};

fn convert_user_part(part: &UserContent) -> Option<Value> {
    match part {
        UserContent::Text(t) => {
            let trimmed = t.text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(json!({ "type": "text", "text": t.text }))
            }
        }
        UserContent::Image(img) => Some(image_json(img)),
        UserContent::ToolResult(result) => Some(json!({
            "type": "tool_result",
            "tool_use_id": result.call,
            "content": result.text(),
        })),
    }
}

fn convert_claude_user(content: &[UserContent]) -> Vec<Value> {
    content.iter().filter_map(convert_user_part).collect()
}

/// Thinking must be replayed byte-for-byte with its signature; unsigned reasoning may be omitted:
/// <https://platform.claude.com/docs/en/build-with-claude/thinking#preserving-thinking-blocks>
fn convert_assistant_reasoning(reasoning: &Reasoning) -> Option<Value> {
    match reasoning.content.first()? {
        ReasoningContent::Text {
            text,
            signature: Some(signature),
        } => Some(json!({ "type": "thinking", "thinking": text, "signature": signature })),
        ReasoningContent::Redacted { data } => Some(json!({ "type": "redacted_thinking", "data": data })),
        _ => None,
    }
}

fn convert_assistant_part(part: &AssistantContent) -> Option<Value> {
    match part {
        AssistantContent::Reasoning(r) => convert_assistant_reasoning(r),
        AssistantContent::Text(t) => {
            if t.text.trim().is_empty() {
                None
            } else {
                Some(json!({ "type": "text", "text": t.text }))
            }
        }
        AssistantContent::ToolCall(call) => Some(json!({
            "type": "tool_use",
            "id": call.id,
            "name": super::to_claude_tool_name(&call.function.name),
            "input": call.function.arguments,
        })),
        _ => None,
    }
}

fn convert_claude_assistant(content: &[AssistantContent]) -> Vec<Value> {
    content.iter().filter_map(convert_assistant_part).collect()
}

pub fn convert_messages(request: &ModelCompletionRequest) -> Vec<Value> {
    let mut turns: Vec<(String, Vec<Value>)> = Vec::new();

    for message in request.messages.iter() {
        match message {
            ChatMessage::System { content } => {
                append_turn(&mut turns, "user", vec![json!({ "type": "text", "text": content })])
            }
            ChatMessage::User { content } => {
                let parts = convert_claude_user(content);
                append_turn(&mut turns, "user", parts);
            }
            ChatMessage::Assistant { content, .. } => {
                let parts = convert_claude_assistant(content);
                append_turn(&mut turns, "assistant", parts);
            }
        }
    }

    if !request.prompt.trim().is_empty() {
        append_turn(
            &mut turns,
            "user",
            vec![json!({ "type": "text", "text": request.prompt.trim() })],
        );
    }

    if turns.first().is_some_and(|(role, _)| role == "assistant") {
        turns.insert(0, ("user".to_string(), vec![json!({"type": "text", "text": "Hello"})]));
    }

    turns
        .into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect()
}

fn append_turn(turns: &mut Vec<(String, Vec<Value>)>, role: &str, mut parts: Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    if let Some((last_role, last_parts)) = turns.last_mut()
        && last_role == role
    {
        last_parts.append(&mut parts);
    } else {
        turns.push((role.to_string(), parts));
    }
}

fn image_json(image: &ImageContent) -> Value {
    let media_type = image.media_type_or_png();
    json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": image.data,
        }
    })
}
