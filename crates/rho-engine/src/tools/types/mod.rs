use rho_harness_core::error::AppError;
use rig::completion::message::{ImageMediaType, MimeType, ToolResultContent};
use rig::tool::ToolExecutionError;
use serde::{Deserialize, Serialize};

mod schema;

#[cfg(test)]
mod tests;

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
    pub image: Option<ToolImage>,
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
            image: Some(image),
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

pub fn into_rig_result(result: Result<ToolResult, AppError>) -> Result<String, ToolExecutionError> {
    match result {
        Ok(result) if result.is_error => Err(ToolExecutionError::other(result.content)),
        Ok(result) => Ok(result.content),
        Err(error) => Err(ToolExecutionError::from_error(error)),
    }
}

pub fn into_dynamic_result(result: Result<ToolResult, AppError>) -> Result<rig::tool::ToolOutput, ToolExecutionError> {
    match result {
        Ok(result) if result.is_error => Err(ToolExecutionError::other(result.content)),
        Ok(result) => Ok(tool_output(result)),
        Err(error) => Err(ToolExecutionError::from_error(error)),
    }
}

/// Text results stay a single text block; image-bearing results become
/// `[text, image]` so vision-capable providers receive the inline image.
fn tool_output(result: ToolResult) -> rig::tool::ToolOutput {
    let Some(image) = result.image else {
        return rig::tool::ToolOutput::text(result.content);
    };
    let media_type = ImageMediaType::from_mime_type(&image.mime);
    rig::tool::ToolOutput::content(vec![
        ToolResultContent::text(result.content),
        ToolResultContent::image_base64(image.data, media_type, None),
    ])
    .expect("a text block plus an image block is never empty")
}
