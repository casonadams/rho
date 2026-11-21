use serde::{Deserialize, Serialize};

mod args;
mod schema;

pub use args::{normalize_tool_arguments, parse_tool_args, resolve_tool_path, try_parse_tool_args};
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
    fn new(content: impl Into<String>, is_error: bool) -> Self {
        Self {
            content: content.into(),
            is_error,
            metadata: None,
            image: None,
            display: None,
            ends_turn: false,
        }
    }

    pub fn success(content: impl Into<String>) -> Self {
        Self::new(content, false)
    }

    /// Successful result whose model-visible output is `content` followed by an
    /// inline image block.
    pub fn success_with_image(content: impl Into<String>, image: ToolImage) -> Self {
        Self {
            image: Some(Box::new(image)),
            ..Self::success(content)
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self::new(content, true)
    }

    pub fn with_metadata(self, metadata: serde_json::Value) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_only_their_own_fields() {
        let ok = ToolResult::success("fine");
        assert_eq!((ok.content.as_str(), ok.is_error), ("fine", false));
        assert!(ok.metadata.is_none() && ok.image.is_none() && ok.display.is_none() && !ok.ends_turn);

        let err = ToolResult::error("bad");
        assert_eq!((err.content.as_str(), err.is_error), ("bad", true));

        let with_meta = ToolResult::success("x").with_metadata(serde_json::json!({ "k": 1 }));
        assert_eq!(with_meta.metadata, Some(serde_json::json!({ "k": 1 })));
        assert!(!with_meta.is_error);
    }
}
