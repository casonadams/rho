use super::*;
use crate::mcp::client::{McpContent, McpToolResult};

#[test]
fn test_gateway_status_and_search() {
    let clients = BTreeMap::new();
    let def1 = McpToolDefinition {
        name: "snapshot".to_string(),
        description: Some("Take browser snapshot".to_string()),
        input_schema: json!({}),
    };
    let def2 = McpToolDefinition {
        name: "click".to_string(),
        description: Some("Click element".to_string()),
        input_schema: json!({}),
    };

    let tool_defs = vec![("playwright".to_string(), def1), ("playwright".to_string(), def2)];

    let gateway = McpGateway::new(clients, tool_defs, 1024);

    let search_results = gateway.search("snap");
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0]["name"], "snapshot");

    let desc = gateway.describe("playwright_click").unwrap();
    assert_eq!(desc["name"], "click");
    assert_eq!(desc["server"], "playwright");
}

#[test]
fn test_mcp_tool_result_truncation_and_images() {
    let res = McpToolResult {
        content: vec![
            McpContent {
                kind: "text".to_string(),
                text: Some("hello world this is a long text output".to_string()),
                data: None,
                mime_type: None,
            },
            McpContent {
                kind: "image".to_string(),
                text: None,
                data: Some("iVBORw0KGgoAAAANSUhEUg==".to_string()),
                mime_type: Some("image/png".to_string()),
            },
        ],
        is_error: Some(false),
    };

    let full_text = res.as_text();
    assert!(full_text.contains("hello world"));
    assert!(full_text.contains("[Image: image/png,"));

    let truncated = res.as_text_truncated(10);
    assert!(truncated.contains("output truncated"));
}
