use super::*;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::{BashTool, EditTool, FdTool, ReadTool, WriteTool};
use rig::tool::{ToolContext, ToolErrorKind, ToolSet};

fn tool_set() -> ToolSet {
    let base = std::env::temp_dir();
    let http = HttpClient::new(false).unwrap();
    let mut tools = ToolSet::default();
    tools.add_tool(ReadTool::new(&base));
    tools.add_tool(WriteTool::new(&base));
    tools.add_tool(EditTool::new(&base));
    tools.add_tool(BashTool::new(&base));
    tools.add_tool(FdTool::new(&base));
    tools.add_tool(WebSearchTool::new(
        http.clone(),
        SearchRateLimiter::new(0),
        WebSearchConfig {
            region: "wt-wt".to_string(),
            timeout_sec: 1,
        },
    ));
    tools.add_tool(WebFetchTool::new(
        http,
        FetchCache::new(60, 4),
        WebFetchConfig {
            timeout_sec: 1,
            max_bytes: 1024,
            default_limit: 20,
        },
    ));
    tools
}

#[test]
fn normalize_schema_replaces_boolean_subschemas() {
    let mut schema = serde_json::json!({
        "$defs": {
            "Item": {
                "type": "string"
            }
        },
        "type": "object",
        "properties": {
            "options": {
                "type": ["array", "null"],
                "items": true
            },
            "item": {
                "$ref": "#/$defs/Item"
            },
            "extra": true
        },
        "prefixItems": [true],
        "anyOf": [true, {"type": "string"}]
    });
    normalize_schema(&mut schema);
    assert_eq!(
        schema,
        serde_json::json!({
            "type": "object",
            "properties": {
                "options": {
                    "type": "array",
                    "items": {}
                },
                "item": {
                    "type": "string"
                },
                "extra": {}
            },
            "prefixItems": [{}],
            "anyOf": [{}, {"type": "string"}]
        })
    );
}

#[test]
fn rig_schemas_are_generated_from_typed_arguments() {
    let tools = tool_set();
    let expected = [
        ("read", &["path"][..]),
        ("write", &["content", "path"][..]),
        ("edit", &["edits", "path"][..]),
        ("bash", &["command"][..]),
        ("web_search", &["query"][..]),
        ("web_fetch", &["url"][..]),
    ];

    for (name, required) in expected {
        let definition = tools
            .get_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == name)
            .unwrap();
        let schema_required = definition.parameters["required"].as_array().unwrap();
        for field in required {
            assert!(schema_required.iter().any(|value| value == field), "{name}.{field}");
        }
    }
}

#[tokio::test]
async fn rig_dispatch_rejects_malformed_arguments_for_every_tool() {
    let tools = tool_set();
    for name in ["read", "write", "edit", "bash", "fd", "web_search", "web_fetch"] {
        let result = tools.execute(name, "not json", &mut ToolContext::new()).await;
        assert!(result.is_error_kind(ToolErrorKind::InvalidArgs), "{name}: {result:?}");
    }
    for name in ["read", "write", "edit", "bash", "web_search", "web_fetch"] {
        let result = tools.execute(name, "{}", &mut ToolContext::new()).await;
        assert!(result.is_error_kind(ToolErrorKind::InvalidArgs), "{name}: {result:?}");
    }
}

#[tokio::test]
async fn rig_dispatch_rejects_unknown_tools() {
    let result = tool_set().execute("unknown", "{}", &mut ToolContext::new()).await;
    assert!(result.is_error_kind(ToolErrorKind::NotFound));
}

#[test]
fn dynamic_result_without_image_is_one_text_block() {
    let output = into_dynamic_result(Ok(ToolResult::success("plain"))).unwrap();
    assert_eq!(output.as_text(), Some("plain"));
}

#[test]
fn dynamic_result_with_image_is_text_then_image_block() {
    let result = ToolResult::success_with_image(
        "Read image file [image/png]",
        ToolImage {
            data: "aGk=".to_string(),
            mime: "image/png".to_string(),
        },
    );
    let output = into_dynamic_result(Ok(result)).unwrap();
    let blocks = output.as_content();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0], ToolResultContent::text("Read image file [image/png]"));
    let ToolResultContent::Image(image) = &blocks[1] else {
        panic!("second block must be an image, got {blocks:?}");
    };
    assert_eq!(image.media_type, Some(rig::completion::message::ImageMediaType::PNG));
    assert_eq!(
        image.data,
        rig::completion::message::DocumentSourceKind::Base64("aGk=".to_string())
    );
}

#[test]
fn dynamic_result_maps_known_and_unknown_image_mimes() {
    let output = into_dynamic_result(Ok(ToolResult::success_with_image(
        "x",
        ToolImage {
            data: String::new(),
            mime: "image/webp".to_string(),
        },
    )))
    .unwrap();
    let ToolResultContent::Image(image) = &output.as_content()[1] else {
        panic!("expected image block");
    };
    assert_eq!(image.media_type, Some(rig::completion::message::ImageMediaType::WEBP));

    let output = into_dynamic_result(Ok(ToolResult::success_with_image(
        "x",
        ToolImage {
            data: String::new(),
            mime: "image/x-unknown".to_string(),
        },
    )))
    .unwrap();
    let ToolResultContent::Image(image) = &output.as_content()[1] else {
        panic!("expected image block");
    };
    assert_eq!(image.media_type, None);
}

#[test]
fn dynamic_error_results_never_carry_images() {
    let result = ToolResult::success_with_image(
        "too late",
        ToolImage {
            data: "aGk=".to_string(),
            mime: "image/png".to_string(),
        },
    );
    let error = into_dynamic_result(Ok(ToolResult {
        is_error: true,
        ..result
    }))
    .unwrap_err();
    assert!(error.to_string().contains("too late"));
}

#[test]
fn tool_result_deserializes_without_image_field() {
    let result: ToolResult = serde_json::from_str(r#"{"content":"legacy","is_error":false}"#).unwrap();
    assert_eq!(result.content, "legacy");
    assert!(result.image.is_none());

    let with_image = ToolResult::success_with_image(
        "note",
        ToolImage {
            data: "aGk=".to_string(),
            mime: "image/gif".to_string(),
        },
    );
    let json = serde_json::to_string(&with_image).unwrap();
    let round_tripped: ToolResult = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped.image.as_ref().unwrap().mime, "image/gif");
}
