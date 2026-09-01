use super::client::McpClient;
use super::process::McpProcess;
use super::transport::McpTransport;
use rho_core::config::Config;
use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use std::path::Path;
use std::sync::Arc;

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<DynamicTool> {
    if !config.mcp.enabled {
        return Vec::new();
    }

    let mut dynamic_tools = Vec::new();

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

        for tool in tools {
            let tool_name = format!("{}_{}", server_name, tool.name);
            let description = tool.description.unwrap_or_default();
            let mut schema = tool.input_schema;
            crate::tools::normalize_schema(&mut schema);
            let client = Arc::clone(&client);
            let original_name = tool.name.clone();

            let dynamic_tool = DynamicTool::new(tool_name, description, schema, move |_ctx, args| {
                let client = Arc::clone(&client);
                let original_name = original_name.clone();
                Box::pin(async move {
                    let result = client
                        .call_tool(&original_name, args)
                        .await
                        .map_err(|e| ToolExecutionError::other(e.to_string()))?;
                    let text = result.as_text();
                    if result.is_error.unwrap_or(false) {
                        Err(ToolExecutionError::other(text))
                    } else {
                        Ok(ToolOutput::text(text))
                    }
                })
            });

            dynamic_tools.push(dynamic_tool);
        }
    }

    dynamic_tools
}
