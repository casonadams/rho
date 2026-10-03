//! Anti-Corruption Layer: translates between Rho domain types and Rig framework types.

pub mod context;
pub mod memory;
pub mod model;
pub use memory::RigSessionMemory;

use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, Reasoning, ReasoningContent, TextContent, ToolCall, ToolFunction,
    ToolResult, ToolResultContent, UserContent,
};
use rig::message::{
    AssistantContent as RigAssistantContent, CallId as RigCallId, Image as RigImage, ImageMediaType as RigMediaType,
    Message as RigMessage, Reasoning as RigReasoning, ReasoningContent as RigReasoningContent, Text as RigText,
    ToolCall as RigToolCall, ToolFunction as RigToolFunction, ToolResult as RigToolResult,
    ToolResultContent as RigToolResultContent, UserContent as RigUserContent,
};

/// Convert a borrowed domain `ChatMessage` into an owned `rig::message::Message`.
pub fn to_rig_message(msg: &ChatMessage) -> RigMessage {
    match msg {
        ChatMessage::System { content } => RigMessage::System {
            content: content.clone(),
        },
        ChatMessage::User { content } => RigMessage::User {
            content: content.iter().map(to_rig_user_content).collect(),
        },
        ChatMessage::Assistant { id, content } => RigMessage::Assistant {
            id: id.clone(),
            content: content.iter().map(to_rig_assistant_content).collect(),
        },
    }
}

/// Convert an owned domain `ChatMessage` into an owned `rig::message::Message`.
pub fn into_rig_message(msg: ChatMessage) -> RigMessage {
    to_rig_message(&msg)
}

/// Convert a borrowed `rig::message::Message` into an owned domain `ChatMessage`.
pub fn from_rig_message(msg: &RigMessage) -> ChatMessage {
    match msg {
        RigMessage::System { content } => ChatMessage::System {
            content: content.clone(),
        },
        RigMessage::User { content } => ChatMessage::User {
            content: content.iter().map(from_rig_user_content).collect(),
        },
        RigMessage::Assistant { id, content } => ChatMessage::Assistant {
            id: id.clone(),
            content: content.iter().map(from_rig_assistant_content).collect(),
        },
    }
}

/// Convert an owned `rig::message::Message` into an owned domain `ChatMessage`.
pub fn into_rho_message(msg: RigMessage) -> ChatMessage {
    from_rig_message(&msg)
}

fn to_rig_user_content(item: &UserContent) -> RigUserContent {
    match item {
        UserContent::Text(t) => RigUserContent::Text(RigText::new(&t.text)),
        UserContent::ToolResult(res) => RigUserContent::ToolResult(to_rig_tool_result(res)),
        UserContent::Image(img) => RigUserContent::Image(to_rig_image(img)),
    }
}

fn from_rig_user_content(item: &RigUserContent) -> UserContent {
    match item {
        RigUserContent::Text(t) => UserContent::Text(TextContent::new(&t.text)),
        RigUserContent::ToolResult(res) => UserContent::ToolResult(ToolResult {
            call: res.call.to_string(),
            provider: None,
            name: res.name.to_string(),
            content: res.content.iter().map(from_rig_tool_result_content).collect(),
        }),
        RigUserContent::Image(img) => UserContent::Image(from_rig_image(img)),
        _ => UserContent::Text(TextContent::new("")),
    }
}

fn to_rig_assistant_content(item: &AssistantContent) -> RigAssistantContent {
    match item {
        AssistantContent::Text(t) => RigAssistantContent::Text(RigText::new(&t.text)),
        AssistantContent::ToolCall(call) => RigAssistantContent::ToolCall(to_rig_tool_call(call)),
        AssistantContent::Reasoning(r) => RigAssistantContent::Reasoning(rig::message::Sealed::new(
            rig::message::Issuer::from_static("rho"),
            to_rig_reasoning(r),
        )),
        AssistantContent::Image(img) => RigAssistantContent::Image(to_rig_image(img)),
    }
}

fn from_rig_assistant_content(item: &RigAssistantContent) -> AssistantContent {
    match item {
        RigAssistantContent::Text(t) => AssistantContent::Text(TextContent::new(&t.text)),
        RigAssistantContent::ToolCall(call) => AssistantContent::ToolCall(from_rig_tool_call(call)),
        RigAssistantContent::Reasoning(r) => {
            let reasoning = r.open(r.issuer());
            AssistantContent::Reasoning(Reasoning {
                id: reasoning.and_then(|r| r.id.clone()),
                content: reasoning
                    .map(|r| r.content.iter().map(from_rig_reasoning_content).collect())
                    .unwrap_or_default(),
            })
        }
        RigAssistantContent::Image(img) => AssistantContent::Image(from_rig_image(img)),
    }
}

fn to_rig_tool_call(call: &ToolCall) -> RigToolCall {
    let tool_name = rig::message::ToolName::new(&call.function.name)
        .unwrap_or_else(|_| rig::message::ToolName::new("unknown").unwrap());
    let mut tc = RigToolCall::new(
        RigCallId::from_wire(&call.id),
        RigToolFunction::new(tool_name, call.function.arguments.clone()),
    );
    tc.signature = call.signature.clone();
    tc
}

fn from_rig_tool_call(call: &RigToolCall) -> ToolCall {
    let mut tc = ToolCall::new(
        call.id.wire().as_ref(),
        ToolFunction::new(call.function.name.as_str(), call.function.arguments.clone()),
    );
    tc.signature = call.signature.clone();
    tc
}

fn to_rig_tool_result(res: &ToolResult) -> RigToolResult {
    let tool_name =
        rig::message::ToolName::new(&res.name).unwrap_or_else(|_| rig::message::ToolName::new("unknown").unwrap());
    RigToolResult {
        call: RigCallId::from_wire(&res.call),
        name: tool_name,
        content: res.content.iter().map(to_rig_tool_result_content).collect(),
    }
}

fn to_rig_tool_result_content(item: &ToolResultContent) -> RigToolResultContent {
    match item {
        ToolResultContent::Text(t) => RigToolResultContent::Text(RigText::new(&t.text)),
        ToolResultContent::Image(img) => RigToolResultContent::Image(to_rig_image(img)),
        ToolResultContent::Json { value } => RigToolResultContent::Json { value: value.clone() },
    }
}

fn from_rig_tool_result_content(item: &RigToolResultContent) -> ToolResultContent {
    match item {
        RigToolResultContent::Text(t) => ToolResultContent::Text(TextContent::new(&t.text)),
        RigToolResultContent::Image(img) => ToolResultContent::Image(from_rig_image(img)),
        RigToolResultContent::Json { value } => ToolResultContent::Json { value: value.clone() },
    }
}

fn to_rig_reasoning(r: &Reasoning) -> RigReasoning {
    let mut rr = RigReasoning {
        id: r.id.clone(),
        content: r.content.iter().map(to_rig_reasoning_content).collect(),
    };
    if let Some(id) = &r.id {
        rr = rr.with_id(id.clone());
    }
    rr
}

fn to_rig_reasoning_content(c: &ReasoningContent) -> RigReasoningContent {
    match c {
        ReasoningContent::Text { text, signature } => RigReasoningContent::Text {
            text: text.clone(),
            signature: signature.clone(),
        },
        ReasoningContent::Summary(s) => RigReasoningContent::Summary(s.clone()),
        ReasoningContent::Redacted { data } => RigReasoningContent::Redacted { data: data.clone() },
        ReasoningContent::Encrypted(e) => RigReasoningContent::Encrypted(e.clone()),
    }
}

fn from_rig_reasoning_content(c: &RigReasoningContent) -> ReasoningContent {
    match c {
        RigReasoningContent::Text { text, signature } => ReasoningContent::Text {
            text: text.clone(),
            signature: signature.clone(),
        },
        RigReasoningContent::Summary(s) => ReasoningContent::Summary(s.clone()),
        RigReasoningContent::Redacted { data } => ReasoningContent::Redacted { data: data.clone() },
        RigReasoningContent::Encrypted(e) => ReasoningContent::Encrypted(e.clone()),
    }
}

fn to_rig_image(img: &ImageContent) -> RigImage {
    RigImage {
        data: rig::message::DocumentSourceKind::Base64(img.data.clone()),
        media_type: img.media_type.as_deref().and_then(parse_media_type),
        detail: None,
        additional_params: None,
    }
}

fn from_rig_image(img: &RigImage) -> ImageContent {
    let data = match &img.data {
        rig::message::DocumentSourceKind::Base64(s) => s.clone(),
        rig::message::DocumentSourceKind::Url(s) => s.clone(),
        other => other.to_string(),
    };
    let media_type = img
        .media_type
        .as_ref()
        .map(|m| crate::antigravity::request::contents::image_mime_type(Some(m)).to_string());
    ImageContent { data, media_type }
}

fn parse_media_type(s: &str) -> Option<RigMediaType> {
    let clean = s.strip_prefix("image/").unwrap_or(s).to_ascii_lowercase();
    serde_json::from_str(&format!("\"{clean}\"")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_and_assistant_message_roundtrip() {
        let user_msg = ChatMessage::user("Please analyze this function");
        let rig_msg = to_rig_message(&user_msg);
        let back_msg = from_rig_message(&rig_msg);
        assert_eq!(user_msg, back_msg);

        let asst_msg = ChatMessage::assistant("The analysis is complete");
        let rig_asst = to_rig_message(&asst_msg);
        let back_asst = from_rig_message(&rig_asst);
        assert_eq!(asst_msg, back_asst);

        let sys_msg = ChatMessage::system("You are a helpful coding assistant.");
        let rig_sys = to_rig_message(&sys_msg);
        let back_sys = from_rig_message(&rig_sys);
        assert_eq!(sys_msg, back_sys);
    }

    #[test]
    fn test_tool_call_and_result_roundtrip() {
        let call = ToolCall::new(
            "call_test_1",
            ToolFunction::new("bash", serde_json::json!({"command": "cargo test"})),
        );
        let asst_msg = ChatMessage::Assistant {
            id: Some("asst_1".to_string()),
            content: vec![AssistantContent::ToolCall(call)],
        };
        let rig_asst = to_rig_message(&asst_msg);
        let back_asst = from_rig_message(&rig_asst);
        assert_eq!(asst_msg, back_asst);

        let res = ToolResult::new("call_test_1", "bash", "All tests passed");
        let user_msg = ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        };
        let rig_user = to_rig_message(&user_msg);
        let back_user = from_rig_message(&rig_user);
        assert_eq!(user_msg, back_user);
    }

    #[test]
    fn test_reasoning_roundtrip() {
        let reasoning = Reasoning::new_with_signature("Let's plan this step", Some("sig_xyz".to_string()));
        let msg = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::Reasoning(reasoning)],
        };
        let rig_msg = to_rig_message(&msg);
        let back_msg = from_rig_message(&rig_msg);
        assert_eq!(msg, back_msg);
    }

    #[test]
    fn test_image_and_multiturn_roundtrip() {
        let img = ImageContent {
            data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
                .to_string(),
            media_type: Some("image/png".to_string()),
        };
        let user_msg = ChatMessage::User {
            content: vec![UserContent::Image(img)],
        };
        let rig_msg = to_rig_message(&user_msg);
        let back_msg = from_rig_message(&rig_msg);
        assert_eq!(user_msg, back_msg);
    }
}
