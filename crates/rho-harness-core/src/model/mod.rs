//! Domain chat message and tool calling primitives.
//!
//! Provides framework-independent types for conversation history, tool calls,
//! tool results, and reasoning blocks. These types form the boundary of the
//! Anti-Corruption Layer, isolating Rho's core logic from third-party SDK types.

use serde::{Deserialize, Serialize};

/// Role-tagged chat message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum ChatMessage {
    /// System message containing instructions.
    System { content: String },

    /// User message containing one or more content blocks.
    User { content: Vec<UserContent> },

    /// Assistant response containing text, tool calls, or reasoning.
    Assistant {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        content: Vec<AssistantContent>,
    },
}

impl ChatMessage {
    /// Create a user message from text.
    pub fn user(text: impl Into<String>) -> Self {
        Self::User {
            content: vec![UserContent::Text(TextContent::new(text))],
        }
    }

    /// Create an assistant message from text.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::Assistant {
            id: None,
            content: vec![AssistantContent::Text(TextContent::new(text))],
        }
    }

    /// Create a system message from text.
    pub fn system(text: impl Into<String>) -> Self {
        Self::System { content: text.into() }
    }

    /// Create a user message with multiple content blocks.
    pub fn user_blocks(content: Vec<UserContent>) -> Self {
        Self::User { content }
    }

    /// Create an assistant message with multiple content blocks.
    pub fn assistant_blocks(content: Vec<AssistantContent>) -> Self {
        Self::Assistant { id: None, content }
    }

    /// Extract text content from the message, if available.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::System { content } => Some(content.as_str()),
            Self::User { content } => content.iter().find_map(|c| match c {
                UserContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            }),
            Self::Assistant { content, .. } => content.iter().find_map(|c| match c {
                AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            }),
        }
    }

    /// Return true if this message is from the user.
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User { .. })
    }

    /// Return true if this message is from the assistant.
    pub fn is_assistant(&self) -> bool {
        matches!(self, Self::Assistant { .. })
    }

    /// Return true if this message is a system message.
    pub fn is_system(&self) -> bool {
        matches!(self, Self::System { .. })
    }
}

/// Content block in a user message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UserContent {
    /// Literal text content.
    Text(TextContent),
    /// Result of an executed tool call.
    ToolResult(ToolResult),
    /// Image content.
    Image(ImageContent),
}

/// Content block in an assistant message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AssistantContent {
    /// Literal text response.
    Text(TextContent),
    /// Tool invocation requested by the assistant.
    ToolCall(ToolCall),
    /// Structured reasoning/thinking emitted by the assistant.
    Reasoning(Reasoning),
    /// Image content emitted by the assistant.
    Image(ImageContent),
}

/// Text payload for a content block.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TextContent {
    pub text: String,
}

impl TextContent {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl From<&str> for TextContent {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for TextContent {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// Function invocation parameters in a tool call.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolFunction {
    pub fn new(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

/// Tool call request from the model.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Value>,
    pub function: ToolFunction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, function: ToolFunction) -> Self {
        let id = id.into();
        Self {
            id,
            function,
            provider: None,
            signature: None,
        }
    }
}

/// Result returned from executing a tool call.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolResult {
    pub call: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Value>,
    pub name: String,
    pub content: Vec<ToolResultContent>,
}

impl ToolResult {
    pub fn new(call: impl Into<String>, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            call: call.into(),
            provider: None,
            name: name.into(),
            content: vec![ToolResultContent::Text(TextContent::new(text))],
        }
    }
}

/// Typed content item within a tool result.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolResultContent {
    /// Literal text output.
    Text(TextContent),
    /// Image output from a tool.
    Image(ImageContent),
    /// Structured JSON payload.
    Json { value: serde_json::Value },
}

impl ToolResultContent {
    pub fn as_text(&self) -> Option<&str> {
        if let Self::Text(t) = self {
            Some(t.text.as_str())
        } else {
            None
        }
    }
}

/// Assistant reasoning block.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Reasoning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub content: Vec<ReasoningContent>,
}

impl Reasoning {
    pub fn new(input: impl Into<String>) -> Self {
        Self::new_with_signature(input, None)
    }

    pub fn new_with_signature(input: impl Into<String>, signature: Option<String>) -> Self {
        Self {
            id: None,
            content: vec![ReasoningContent::Text {
                text: input.into(),
                signature,
            }],
        }
    }

    pub fn display_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ReasoningContent::Text { text, .. } => Some(text.as_str()),
                ReasoningContent::Summary(s) => Some(s.as_str()),
                ReasoningContent::Redacted { data } => Some(data.as_str()),
                ReasoningContent::Encrypted(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn first_text_and_signature(&self) -> (Option<&str>, Option<&str>) {
        for c in &self.content {
            if let ReasoningContent::Text { text, signature } = c {
                return (Some(text.as_str()), signature.as_deref());
            }
        }
        (None, None)
    }

    pub fn first_text(&self) -> Option<&str> {
        self.first_text_and_signature().0
    }

    pub fn first_signature(&self) -> Option<&str> {
        self.first_text_and_signature().1
    }
}

/// Typed reasoning block variant.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ReasoningContent {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    Encrypted(String),
    Redacted {
        data: String,
    },
    Summary(String),
}

/// Image content block.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImageContent {
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_constructors() {
        let u = ChatMessage::user("hello");
        assert!(u.is_user());
        assert_eq!(u.text(), Some("hello"));

        let a = ChatMessage::assistant("world");
        assert!(a.is_assistant());
        assert_eq!(a.text(), Some("world"));

        let s = ChatMessage::system("instructions");
        assert!(s.is_system());
        assert_eq!(s.text(), Some("instructions"));
    }

    #[test]
    fn test_chat_message_wire_format() {
        let msg = ChatMessage::user("echo test");
        let val = serde_json::to_value(&msg).unwrap();
        assert_eq!(val["role"], "user");
        assert_eq!(val["content"][0]["type"], "text");
        assert_eq!(val["content"][0]["text"], "echo test");

        let back: ChatMessage = serde_json::from_value(val).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn test_tool_call_and_result_serde() {
        let call = ToolCall::new(
            "call_123",
            ToolFunction::new("bash", serde_json::json!({"command": "cargo check"})),
        );
        let msg = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"assistant""#));
        assert!(json.contains(r#""type":"toolcall""#));
        assert!(json.contains(r#""name":"bash""#));

        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);

        let result = ToolResult::new("call_123", "bash", "Finished with 0");
        let user_msg = ChatMessage::User {
            content: vec![UserContent::ToolResult(result)],
        };
        let user_json = serde_json::to_string(&user_msg).unwrap();
        assert!(user_json.contains(r#""type":"toolresult""#));

        let parsed_user: ChatMessage = serde_json::from_str(&user_json).unwrap();
        assert_eq!(parsed_user, user_msg);
    }

    #[test]
    fn test_reasoning_block() {
        let r = Reasoning::new_with_signature("thinking step", Some("sig_abc".to_string()));
        assert_eq!(r.first_text(), Some("thinking step"));
        assert_eq!(r.first_signature(), Some("sig_abc"));
        assert_eq!(r.display_text(), "thinking step");

        let msg = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::Reasoning(r)],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }
}
