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
        let result = tools
            .execute(name, "{\"unexpected\":true}", &mut ToolContext::new())
            .await;
        assert!(result.is_error_kind(ToolErrorKind::InvalidArgs), "{name}: {result:?}");
    }
}

#[tokio::test]
async fn rig_dispatch_rejects_unknown_tools() {
    let result = tool_set().execute("unknown", "{}", &mut ToolContext::new()).await;
    assert!(result.is_error_kind(ToolErrorKind::NotFound));
}
