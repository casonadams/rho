use super::runtime::McpRuntime;
use super::search::ServerState;
use crate::tools::{EngineTool, GenericTool, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

pub const GATEWAY_TOOL_NAME: &str = "mcp";

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn unknown_tool(runtime: &McpRuntime, server: &str, tool: &str) -> ToolResult {
    let state = match runtime.catalog().server_states().get(server) {
        Some(ServerState::Connecting) => " The server is still connecting; retry shortly.".to_string(),
        Some(ServerState::Unavailable(msg)) => format!(" The server is unavailable: {msg}"),
        Some(ServerState::Stale(_)) | None => String::new(),
    };
    ToolResult::error(format!(
        "Unknown MCP tool '{tool}' on server '{server}'. Use tool_search to find available tools.{state}"
    ))
}

async fn call(runtime: &McpRuntime, server: &str, args: &Value) -> ToolResult {
    let Some(tool) = str_arg(args, "tool") else {
        return ToolResult::error("The `tool` field is required to call an MCP tool.");
    };
    let Some(entry) = runtime.catalog().find(Some(server), tool) else {
        return unknown_tool(runtime, server, tool);
    };
    let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    runtime.call_tool(&entry.server_name, &entry.name, arguments).await
}

async fn execute(runtime: &McpRuntime, args: &Value) -> ToolResult {
    let Some(server) = str_arg(args, "server") else {
        return ToolResult::error("The `server` field is required.");
    };
    if runtime.server_config(server).is_none() {
        let known: Vec<&str> = runtime.server_names().collect();
        return ToolResult::error(format!(
            "Unknown MCP server '{server}'. Configured servers: {}",
            known.join(", ")
        ));
    }
    match str_arg(args, "action").unwrap_or("call") {
        "call" => call(runtime, server, args).await,
        "list_resources" => runtime.list_resources(server).await,
        "read_resource" => match str_arg(args, "uri") {
            Some(uri) => runtime.read_resource(server, uri).await,
            None => ToolResult::error("The `uri` field is required for read_resource."),
        },
        other => ToolResult::error(format!(
            "Unknown action '{other}'. Use call, list_resources, or read_resource."
        )),
    }
}

pub fn build_gateway_tool(runtime: Arc<McpRuntime>) -> Arc<dyn EngineTool> {
    let description = "Call MCP server tools found with tool_search ({server, tool, arguments}), \
                       and list or read MCP server resources (action list_resources / read_resource with uri).";
    let schema = json!({
        "type": "object",
        "properties": {
            "server": { "type": "string", "description": "MCP server name" },
            "tool": { "type": "string", "description": "Tool name on that server, as listed by tool_search" },
            "arguments": { "type": "object", "description": "Arguments matching the tool's input schema" },
            "action": {
                "type": "string",
                "enum": ["call", "list_resources", "read_resource"],
                "description": "What to do (default: call)"
            },
            "uri": { "type": "string", "description": "Resource URI for read_resource" }
        },
        "required": ["server"]
    });
    Arc::new(GenericTool::new(GATEWAY_TOOL_NAME, description, schema, move |args| {
        let runtime = Arc::clone(&runtime);
        async move { Ok(execute(&runtime, &args).await) }
    }))
}
