//! Content and message conversions for Claude Messages API requests.

use base64::Engine;
use rig::completion::CompletionRequest;
use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::{Value, json};

pub fn convert_messages(request: &CompletionRequest) -> Vec<Value> {
    let mut messages: Vec<(String, Vec<Value>)> = Vec::new();
    for message in &request.chat_history {
        match message {
            Message::System { .. } => {}
            Message::User { content } => {
                let parts: Vec<Value> = content.iter().filter_map(convert_user_content).collect();
                append_turn(&mut messages, "user", parts);
            }
            Message::Assistant { content, .. } => {
                let parts: Vec<Value> = content.iter().filter_map(convert_assistant_content).collect();
                append_turn(&mut messages, "assistant", parts);
            }
        }
    }

    if messages.first().is_some_and(|(role, _)| role == "assistant") {
        messages.insert(0, ("user".to_string(), vec![json!({"type": "text", "text": "Hello"})]));
    }

    messages
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

fn convert_user_content(item: &UserContent) -> Option<Value> {
    match item {
        UserContent::Text(text) if !text.text.trim().is_empty() => Some(json!({ "type": "text", "text": text.text })),
        UserContent::ToolResult(result) => {
            let text = result
                .content
                .iter()
                .filter_map(|c| match c {
                    ToolResultContent::Text(t) => Some(t.text.as_str()),
                    ToolResultContent::Json { value } => value.as_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(json!({
                "type": "tool_result",
                "tool_use_id": result.call.to_string(),
                "content": text,
            }))
        }
        UserContent::Image(image) => {
            let data = match &image.data {
                rig::message::DocumentSourceKind::Base64(b64) => b64.clone(),
                rig::message::DocumentSourceKind::Raw(bytes) => base64::engine::general_purpose::STANDARD.encode(bytes),
                _ => return None,
            };
            Some(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type.as_ref().map(image_mime).unwrap_or("image/png"),
                    "data": data,
                }
            }))
        }
        _ => None,
    }
}

fn convert_assistant_content(item: &AssistantContent) -> Option<Value> {
    match item {
        AssistantContent::Text(text) if !text.text.trim().is_empty() => {
            Some(json!({ "type": "text", "text": text.text }))
        }
        AssistantContent::Reasoning(reasoning) => {
            let block = reasoning.content.first()?;
            if let rig::message::ReasoningContent::Text { text, signature } = block
                && let Some(sig) = signature
            {
                Some(json!({ "type": "thinking", "thinking": text, "signature": sig }))
            } else {
                None
            }
        }
        AssistantContent::ToolCall(call) => Some(json!({
            "type": "tool_use",
            "id": call.id.to_string(),
            "name": call.function.name,
            "input": call.function.arguments,
        })),
        _ => None,
    }
}

fn image_mime(media: &rig::message::ImageMediaType) -> &'static str {
    match media {
        rig::message::ImageMediaType::JPEG => "image/jpeg",
        rig::message::ImageMediaType::PNG => "image/png",
        rig::message::ImageMediaType::GIF => "image/gif",
        rig::message::ImageMediaType::WEBP => "image/webp",
        _ => "image/png",
    }
}
