use crate::adapter::rig::tools::{into_dynamic_result, into_dynamic_tool};
use crate::tools::types::{ToolImage, ToolResult};
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::{BashTool, EditTool, FdTool, ReadTool, WriteTool};
use rig::tool::{DynamicTool, ToolContext, ToolErrorKind, ToolSet};

fn add_core_tools(tools: &mut Vec<DynamicTool>, base: &std::path::Path) {
    tools.push(into_dynamic_tool(ReadTool::new(base)));
    tools.push(into_dynamic_tool(WriteTool::new(base)));
    tools.push(into_dynamic_tool(EditTool::new(base)));
    tools.push(into_dynamic_tool(BashTool::new(base)));
    tools.push(into_dynamic_tool(FdTool::new(base)));
}

fn add_web_tools(tools: &mut Vec<DynamicTool>, http: HttpClient) {
    tools.push(into_dynamic_tool(WebSearchTool::new(
        http.clone(),
        SearchRateLimiter::new(0),
        WebSearchConfig {
            region: "wt-wt".to_string(),
            timeout_sec: 1,
            engines: Vec::new(),
        },
    )));
    tools.push(into_dynamic_tool(WebFetchTool::new(
        http,
        FetchCache::new(60, 4),
        WebFetchConfig {
            timeout_sec: 1,
            max_bytes: 1024,
            pdf_max_bytes: 1024,
            default_limit: 10,
            multimodal: false,
            auth_file: None,
        },
    )));
}

fn tool_set() -> ToolSet {
    let temp = tempfile::tempdir().unwrap();
    let http = HttpClient::new(false).unwrap();
    let mut tools = Vec::new();
    add_core_tools(&mut tools, temp.path());
    add_web_tools(&mut tools, http);
    ToolSet::from_dynamic_tools(tools)
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
            .tool_definitions()
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
    assert_eq!(
        blocks[0],
        rig::completion::message::ToolResultContent::text("Read image file [image/png]")
    );
    let rig::completion::message::ToolResultContent::Image(image) = &blocks[1] else {
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
    let rig::completion::message::ToolResultContent::Image(image) = &output.as_content()[1] else {
        panic!("expected image block");
    };
    assert_eq!(image.media_type, Some(rig::completion::message::ImageMediaType::WEBP));

    let output = into_dynamic_result(Ok(ToolResult::success_with_image(
        "x",
        ToolImage {
            data: String::new(),
            mime: "image/custom".to_string(),
        },
    )))
    .unwrap();
    let rig::completion::message::ToolResultContent::Image(image) = &output.as_content()[1] else {
        panic!("expected image block");
    };
    assert_eq!(image.media_type, None);
}

#[test]
fn dynamic_result_propagates_is_error_flag_as_tool_execution_error() {
    let error = into_dynamic_result(Ok(ToolResult {
        content: "something broke".to_string(),
        is_error: true,
        metadata: None,
        image: None,
    }))
    .unwrap_err();

    assert!(error.to_string().contains("something broke"));
}
