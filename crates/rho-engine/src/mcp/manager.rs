use super::client::McpClient;
use super::process::McpProcess;
use super::transport::McpTransport;
use rho_core::config::Config;
use rig::tool::{DynamicTool, ToolOutput};
use std::path::Path;
use std::sync::Arc;

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<DynamicTool> {
    if !config.mcp.enabled {
        return Vec::new();
    }

    let mut dynamic_tools = Vec::new();
    let mut all_clients = std::collections::BTreeMap::new();
    let mut all_tool_defs = Vec::new();

    for (server_name, server_config) in &config.mcp.servers {
        if !server_config.enabled {
            continue;
        }

        let (stdin, stdout, handle) = match McpProcess::spawn(server_config, working_dir) {
            Ok(tuple) => tuple,
            Err(e) => {
                eprintln!("Warning: Failed to spawn MCP server '{server_name}': {e}");
                continue;
            }
        };

        let transport = McpTransport::new(stdin, stdout, handle);
        let client = Arc::new(McpClient::new(server_name, transport));

        if let Err(e) = client.initialize().await {
            eprintln!("Warning: Failed to initialize MCP server '{server_name}': {e}");
            continue;
        }

        let tools = match client.list_tools().await {
            Ok(tools) => tools,
            Err(e) => {
                eprintln!("Warning: Failed to list tools from MCP server '{server_name}': {e}");
                continue;
            }
        };

        all_clients.insert(server_name.clone(), Arc::clone(&client));

        for tool in tools {
            all_tool_defs.push((server_name.clone(), tool.clone()));
            let tool_name = format!("{}_{}", server_name, tool.name);
            let description = format!("[MCP: {server_name}] {}", tool.description.unwrap_or_default());
            let mut schema = tool.input_schema;
            crate::tools::normalize_schema(&mut schema);
            let client = Arc::clone(&client);
            let original_name = tool.name.clone();
            let max_bytes = config.output_max_bytes;

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
    }

    if !all_clients.is_empty() {
        let gateway = super::gateway::McpGateway::new(all_clients, all_tool_defs, config.output_max_bytes);
        let (gw_tool, script_tool) = gateway.into_dynamic_tools();
        dynamic_tools.push(gw_tool);
        dynamic_tools.push(script_tool);
    }

    dynamic_tools
}
