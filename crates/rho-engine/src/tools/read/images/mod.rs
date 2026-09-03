mod resize;
mod sniff;

#[cfg(test)]
mod tests;

pub use sniff::{SNIFF_WINDOW_BYTES, SniffedMime, detect_supported_image_mime};

use crate::tools::types::{ToolImage, ToolResult};
use resize::{ResizedImage, resize_to_limits};
use std::borrow::Cow;

/// Successfully processed image, ready to become a tool-result image block.
#[derive(Debug, PartialEq)]
pub struct ProcessedImage {
    /// Base64-encoded image data.
    pub data: String,
    pub mime: &'static str,
    /// Conversion and dimension notes, in pi's order (conversion first).
    pub hints: Vec<String>,
}

/// Pipeline failure, carrying the model-facing omission message (pi parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    Convert,
    Resize,
}

impl ProcessError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Convert => "[Image omitted: could not be converted to a supported inline image format.]",
            Self::Resize => "[Image omitted: could not be resized below the inline image size limit.]",
        }
    }
}

/// Port of pi's `processImage`: normalize the format (BMP → PNG), run the
/// resize ladder, and assemble pi's hints (conversion + dimension note).
pub fn process_image(bytes: &[u8], mime: &'static str) -> Result<ProcessedImage, ProcessError> {
    let (bytes, mime, converted_from): (Cow<'_, [u8]>, &'static str, Option<&'static str>) = match mime {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => (Cow::Borrowed(bytes), mime, None),
        other => {
            let png = convert_to_png(bytes).ok_or(ProcessError::Convert)?;
            (Cow::Owned(png), "image/png", Some(other))
        }
    };
    let resized = resize_to_limits(&bytes, mime).ok_or(ProcessError::Resize)?;
    let mut hints = Vec::new();
    if let Some(from) = converted_from {
        hints.push(format!("[Image converted from {from} to {mime}.]"));
    }
    if let Some(note) = dimension_note(&resized) {
        hints.push(note);
    }
    Ok(ProcessedImage {
        data: resized.data,
        mime: resized.mime,
        hints,
    })
}

/// pi's read-tool image branch: assemble the model-facing text note and attach
/// the processed image. Processing failures surface pi's omission message as a
/// successful text-only result (no image block).
pub fn tool_result(bytes: &[u8], mime: &'static str) -> ToolResult {
    match process_image(bytes, mime) {
        Ok(processed) => {
            let mut text = format!("Read image file [{}]", processed.mime);
            for hint in &processed.hints {
                text.push('\n');
                text.push_str(hint);
            }
            ToolResult::success_with_image(
                text,
                ToolImage {
                    data: processed.data,
                    mime: processed.mime.to_string(),
                },
            )
        }
        Err(error) => ToolResult::success(format!("Read image file [{mime}]\n{}", error.message())),
    }
}

/// pi's `formatDimensionNote` — helps the model map coordinates back to the
/// original image after downscaling.
pub(crate) fn dimension_note(result: &ResizedImage) -> Option<String> {
    if !result.was_resized {
        return None;
    }
    let scale = f64::from(result.original_width) / f64::from(result.width);
    Some(format!(
        "[Image: original {}x{}, displayed at {}x{}. Multiply coordinates by {scale:.2} to map to original image.]",
        result.original_width, result.original_height, result.width, result.height
    ))
}

/// Re-encode an unsupported-but-decodable image (e.g. BMP) as PNG.
fn convert_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}
