use serde::{Deserialize, Serialize};

mod args;
mod schema;

pub use args::{normalize_tool_arguments, parse_tool_args, try_parse_tool_args};
pub use schema::{generated_schema, normalize_schema};

/// An inline image attached to a successful tool result. The turn runner keeps it
/// for providers that serialize tool-result images and replaces
/// it with an omission note for everyone else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolImage {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type such as "image/png".
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// What the model sees.
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Box<ToolImage>>,
    /// What the user sees when it differs from `content`, such as the same output with terminal colors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// Ends the agent's turn once this call's batch is answered, as if the model had finished.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ends_turn: bool,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
            image: None,
            display: None,
            ends_turn: false,
        }
    }

    /// Successful result whose model-visible output is `content` followed by an
    /// inline image block.
    pub fn success_with_image(content: impl Into<String>, image: ToolImage) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
            image: Some(Box::new(image)),
            display: None,
            ends_turn: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            metadata: None,
            image: None,
            display: None,
            ends_turn: false,
        }
    }
}
