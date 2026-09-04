use super::client::{McpClient, McpToolDefinition};
use super::process::McpProcess;
use super::transport::McpTransport;
use rho_harness_core::config::{Config, McpServerConfig};
use rig::tool::{DynamicTool, ToolOutput};
use std::path::Path;
use std::sync::Arc;

struct ServerLoadTarget<'a> {
    name: &'a str,
    config: &'a McpServerConfig,
    working_dir: &'a Path,
    max_bytes: usize,
}

struct SingleServerLoaded {
    server_name: String,
    client: Arc<McpClient>,
    tool_defs: Vec<(String, McpToolDefinition)>,
    dynamic_tools: Vec<DynamicTool>,
}

async fn load_single_server(target: ServerLoadTarget<'_>) -> Option<SingleServerLoaded> {
    let (stdin, stdout, handle) = match McpProcess::spawn(target.config, target.working_dir) {
        Ok(tuple) => tuple,
        Err(e) => {
            eprintln!("Warning: Failed to spawn MCP server '{}': {e}", target.name);
            return None;
        }
    };

    let transport = McpTransport::new(stdin, stdout, handle);
    let client = Arc::new(McpClient::new(target.name, transport));

    if let Err(e) = client.initialize().await {
        eprintln!("Warning: Failed to initialize MCP server '{}': {e}", target.name);
        return None;
    }

    let tools = match client.list_tools().await {
        Ok(tools) => tools,
        Err(e) => {
            eprintln!("Warning: Failed to list tools from MCP server '{}': {e}", target.name);
            return None;
        }
    };

    let mut tool_defs = Vec::new();
    let mut dynamic_tools = Vec::new();

    for tool in tools {
        tool_defs.push((target.name.to_string(), tool.clone()));
        let tool_name = format!("{}_{}", target.name, tool.name);
        let description = format!("[MCP: {}] {}", target.name, tool.description.unwrap_or_default());
        let mut schema = tool.input_schema;
        crate::tools::normalize_schema(&mut schema);
        let client = Arc::clone(&client);
        let original_name = tool.name.clone();
        let max_bytes = target.max_bytes;

        let dynamic_tool = DynamicTool::new(tool_name, description, schema, move |_ctx, args| {
            let client = Arc::clone(&client);
            let original_name = original_name.clone();
            Box::pin(async move {
                match client.call_tool(&original_name, args).await {
                    Ok(result) => {
                        let text = result.as_text_truncated(max_bytes);
                        if result.is_error.unwrap_or(false) {
                            Ok(ToolOutput::text(format!("[Error] {text}")))
                        } else {
                            Ok(ToolOutput::text(text))
                        }
                    }
                    Err(e) => Ok(ToolOutput::text(format!("[MCP Error] {e}"))),
                }
            })
        });

        dynamic_tools.push(dynamic_tool);
    }

    Some(SingleServerLoaded {
        server_name: target.name.to_string(),
        client,
        tool_defs,
        dynamic_tools,
    })
}

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<DynamicTool> {
    if !config.mcp.enabled {
        return Vec::new();
    }

    let futures: Vec<_> = config
        .mcp
        .servers
        .iter()
        .filter(|(_, cfg)| cfg.enabled)
        .map(|(name, cfg)| {
            load_single_server(ServerLoadTarget {
                name,
                config: cfg,
                working_dir,
                max_bytes: config.output_max_bytes,
            })
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut dynamic_tools = Vec::new();
    let mut all_clients = std::collections::BTreeMap::new();
    let mut all_tool_defs = Vec::new();

    for loaded in results.into_iter().flatten() {
        all_clients.insert(loaded.server_name, loaded.client);
        all_tool_defs.extend(loaded.tool_defs);
        dynamic_tools.extend(loaded.dynamic_tools);
    }

    if !all_clients.is_empty() {
        let gateway = super::gateway::McpGateway::new(all_clients, all_tool_defs, config.output_max_bytes);
        let (gw_tool, script_tool) = gateway.into_dynamic_tools();
        dynamic_tools.push(gw_tool);
        dynamic_tools.push(script_tool);
    }

    dynamic_tools
}
