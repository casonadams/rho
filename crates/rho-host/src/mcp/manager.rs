use super::adapter::McpToolCapability;
use super::client::McpClient;
use super::process::McpProcess;
use super::schema::mcp_tool_to_descriptor;
use super::transport::McpTransport;
use rho_core::config::Config;
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::ToolCapability;
use std::path::Path;
use std::sync::Arc;

pub async fn load_mcp_capabilities(
    config: &Config,
    working_dir: &Path,
) -> Vec<(CapabilityId, Arc<dyn ToolCapability>)> {
    if !config.mcp.enabled {
        return Vec::new();
    }

    let mut capabilities = Vec::new();

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
            let descriptor = mcp_tool_to_descriptor(server_name, &tool);
            let id = descriptor.id.clone();
            let capability = Arc::new(McpToolCapability::new(Arc::clone(&client), tool.name, descriptor));
            capabilities.push((id, capability as Arc<dyn ToolCapability>));
        }
    }

    capabilities
}
