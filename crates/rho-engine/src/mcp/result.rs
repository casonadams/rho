use super::client::{McpContent, McpResourceContents, McpToolResult};
use crate::tools::ToolResult;
use crate::tools::types::ToolImage;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct OutputLimits<'a> {
    pub max_bytes: usize,
    pub artifact_dir: Option<&'a Path>,
}

fn bound(text: String, limits: OutputLimits<'_>) -> String {
    if limits.max_bytes == 0 || text.len() <= limits.max_bytes {
        return text;
    }
    crate::tools::truncate::truncate_head_with_spill(&text, usize::MAX, limits.max_bytes, limits.artifact_dir).content
}

fn omitted_binary(kind: &str, mime: Option<&str>, data: Option<&str>) -> String {
    let mime = mime.unwrap_or("application/octet-stream");
    let len = data.map_or(0, str::len);
    format!("[{kind}: {mime}, {len} bytes base64 omitted]")
}

pub fn describe_resource(resource: &McpResourceContents) -> String {
    match (&resource.text, &resource.blob) {
        (Some(text), _) => format!("[Resource: {}]\n{text}", resource.uri),
        (None, blob) => format!(
            "[Resource: {}] {}",
            resource.uri,
            omitted_binary("Binary", resource.mime_type.as_deref(), blob.as_deref())
        ),
    }
}

fn describe_link(item: &McpContent) -> String {
    let uri = item.uri.as_deref().unwrap_or("");
    let mut out = format!("[Resource link: {uri}]");
    if let Some(name) = item.name.as_deref() {
        out.push(' ');
        out.push_str(name);
    }
    if let Some(desc) = item.description.as_deref() {
        out.push_str(" - ");
        out.push_str(desc);
    }
    out
}

pub fn describe_content(item: &McpContent) -> String {
    match item.kind.as_str() {
        "image" => omitted_binary("Image", item.mime_type.as_deref(), item.data.as_deref()),
        "audio" => omitted_binary("Audio", item.mime_type.as_deref(), item.data.as_deref()),
        "resource" => item
            .resource
            .as_ref()
            .map_or_else(|| "[Resource: missing contents]".to_string(), describe_resource),
        "resource_link" => describe_link(item),
        kind => item
            .text
            .clone()
            .unwrap_or_else(|| format!("[Unsupported MCP content type: {kind}]")),
    }
}

fn text_repeats_structured(content: &[McpContent], structured: &Value) -> bool {
    content
        .iter()
        .filter_map(|c| c.text.as_deref())
        .filter_map(|t| serde_json::from_str::<Value>(t).ok())
        .any(|parsed| &parsed == structured)
}

fn take_inline_image(item: &McpContent) -> Option<ToolImage> {
    (item.kind == "image").then_some(())?;
    Some(ToolImage {
        data: item.data.clone()?,
        mime: item.mime_type.clone().unwrap_or_else(|| "image/png".to_string()),
    })
}

pub fn tool_result(result: &McpToolResult, limits: OutputLimits<'_>) -> ToolResult {
    let mut parts = Vec::new();
    let mut image = None;
    for item in &result.content {
        if image.is_none()
            && let Some(inline) = take_inline_image(item)
        {
            image = Some(inline);
            continue;
        }
        parts.push(describe_content(item));
    }
    if let Some(structured) = &result.structured_content
        && !text_repeats_structured(&result.content, structured)
    {
        let pretty = serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
        parts.push(format!("```json\n{pretty}\n```"));
    }
    let text = bound(parts.join("\n"), limits);
    match (result.is_error.unwrap_or(false), image) {
        (true, _) => ToolResult::error(text),
        (false, Some(image)) => ToolResult::success_with_image(text, image),
        (false, None) => ToolResult::success(text),
    }
}

pub fn resource_result(contents: &[McpResourceContents], limits: OutputLimits<'_>) -> ToolResult {
    if contents.is_empty() {
        return ToolResult::success("[Resource returned no contents]");
    }
    let text = contents.iter().map(describe_resource).collect::<Vec<_>>().join("\n\n");
    ToolResult::success(bound(text, limits))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNBOUNDED: OutputLimits<'static> = OutputLimits {
        max_bytes: 0,
        artifact_dir: None,
    };

    fn text(t: &str) -> McpContent {
        McpContent {
            kind: "text".into(),
            text: Some(t.into()),
            ..Default::default()
        }
    }

    fn image(data: &str) -> McpContent {
        McpContent {
            kind: "image".into(),
            data: Some(data.into()),
            mime_type: Some("image/jpeg".into()),
            ..Default::default()
        }
    }

    #[test]
    fn content_blocks_are_rendered_by_type() {
        let result = McpToolResult {
            content: vec![
                text("hello"),
                McpContent {
                    kind: "resource".into(),
                    resource: Some(McpResourceContents {
                        uri: "file:///a.txt".into(),
                        text: Some("body".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                McpContent {
                    kind: "resource_link".into(),
                    uri: Some("repo://issues/1".into()),
                    name: Some("Issue 1".into()),
                    description: Some("first".into()),
                    ..Default::default()
                },
                McpContent {
                    kind: "audio".into(),
                    data: Some("AAAA".into()),
                    mime_type: Some("audio/wav".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let out = tool_result(&result, UNBOUNDED);
        assert!(!out.is_error);
        assert_eq!(
            out.content,
            "hello\n[Resource: file:///a.txt]\nbody\n[Resource link: repo://issues/1] Issue 1 - first\n[Audio: audio/wav, 4 bytes base64 omitted]"
        );
    }

    #[test]
    fn first_image_is_attached_and_later_ones_are_described() {
        let result = McpToolResult {
            content: vec![text("shot"), image("Zmlyc3Q="), image("c2Vjb25k")],
            ..Default::default()
        };
        let out = tool_result(&result, UNBOUNDED);
        let attached = out.image.expect("image attached");
        assert_eq!(
            (attached.data.as_str(), attached.mime.as_str()),
            ("Zmlyc3Q=", "image/jpeg")
        );
        assert_eq!(out.content, "shot\n[Image: image/jpeg, 8 bytes base64 omitted]");
    }

    #[test]
    fn structured_content_is_appended_unless_text_already_carries_it() {
        let structured = serde_json::json!({"count": 2});
        let only_structured = McpToolResult {
            structured_content: Some(structured.clone()),
            ..Default::default()
        };
        assert_eq!(
            tool_result(&only_structured, UNBOUNDED).content,
            "```json\n{\n  \"count\": 2\n}\n```"
        );
        let duplicated = McpToolResult {
            content: vec![text(r#"{"count":2}"#)],
            structured_content: Some(structured),
            ..Default::default()
        };
        assert_eq!(tool_result(&duplicated, UNBOUNDED).content, r#"{"count":2}"#);
    }

    #[test]
    fn errors_keep_text_and_drop_images() {
        let result = McpToolResult {
            content: vec![text("boom"), image("eA==")],
            is_error: Some(true),
            ..Default::default()
        };
        let out = tool_result(&result, UNBOUNDED);
        assert!(out.is_error);
        assert!(out.image.is_none());
        assert_eq!(out.content, "boom");
    }

    #[test]
    fn oversized_output_is_truncated_and_spilled_to_an_artifact() {
        let dir = crate::test_support::TestDir::new();
        let body = "line\n".repeat(100);
        let result = McpToolResult {
            content: vec![text(&body)],
            ..Default::default()
        };
        let out = tool_result(
            &result,
            OutputLimits {
                max_bytes: 64,
                artifact_dir: Some(&dir),
            },
        );
        assert!(out.content.len() < body.len());
        assert!(out.content.contains("Full content"), "{}", out.content);
        let spilled = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        assert_eq!(std::fs::read_to_string(spilled).unwrap(), body);
    }

    #[test]
    fn resource_reads_render_text_and_binary_contents() {
        let contents = [
            McpResourceContents {
                uri: "memo://a".into(),
                text: Some("note".into()),
                ..Default::default()
            },
            McpResourceContents {
                uri: "memo://b".into(),
                mime_type: Some("application/pdf".into()),
                blob: Some("JVBERg==".into()),
                ..Default::default()
            },
        ];
        assert_eq!(
            resource_result(&contents, UNBOUNDED).content,
            "[Resource: memo://a]\nnote\n\n[Resource: memo://b] [Binary: application/pdf, 8 bytes base64 omitted]"
        );
        assert_eq!(
            resource_result(&[], UNBOUNDED).content,
            "[Resource returned no contents]"
        );
    }
}
