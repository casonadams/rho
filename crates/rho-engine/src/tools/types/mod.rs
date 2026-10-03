use serde::{Deserialize, Serialize};

mod schema;

pub use schema::{generated_schema, normalize_schema};

/// An inline image attached to a successful tool result. The turn hook keeps it
/// for providers whose rig adapter serializes tool-result images and replaces
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
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Box<ToolImage>>,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
            image: None,
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
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            metadata: None,
            image: None,
        }
    }
}
