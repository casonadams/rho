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
    tools: Vec<McpToolDefinition>,
    max_bytes: usize,
    mode: rho_harness_core::config::McpExposureMode,
}

fn is_tool_allowed(name: &str, include: Option<&[String]>, exclude: Option<&[String]>) -> bool {
    if let Some(exc) = exclude
        && exc.iter().any(|p| p == name || p == "*")
    {
        return false;
    }
    if let Some(inc) = include {
        return inc.iter().any(|p| p == name || p == "*");
    }
    true
}

fn build_single_mcp_tool(
    tool: McpToolDefinition,
    client: Arc<McpClient>,
    (server_name, max_bytes): (&str, usize),
) -> DynamicTool {
    let tool_name = format!("{}_{}", server_name, tool.name);
    let description = format!("[MCP: {}] {}", server_name, tool.description.unwrap_or_default());
    let mut schema = tool.input_schema;
    crate::tools::normalize_schema(&mut schema);
    let original_name = tool.name;

    DynamicTool::new(tool_name, description, schema, move |_ctx, args| {
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
    })
}

async fn spawn_and_init_client(target: &ServerLoadTarget<'_>) -> Option<Arc<McpClient>> {
    let timeout = target.config.timeout_seconds.map(std::time::Duration::from_secs);
    let transport = if let Some(url) = &target.config.url {
        let kind = target.config.resolved_transport();
        McpTransport::new_http(url, kind, target.config.headers.clone(), timeout)
    } else {
        let (stdin, stdout, handle) = McpProcess::spawn(target.config, target.working_dir)
            .map_err(|e| eprintln!("Warning: Failed to spawn MCP server '{}': {e}", target.name))
            .ok()?;
        McpTransport::new_stdio(stdin, stdout, handle, timeout)
    };

    let client = Arc::new(McpClient::new(target.name, transport));
    if let Err(e) = client.initialize().await {
        eprintln!("Warning: Failed to initialize MCP server '{}': {e}", target.name);
        return None;
    }
    Some(client)
}

async fn load_single_server(target: ServerLoadTarget<'_>) -> Option<SingleServerLoaded> {
    let client = spawn_and_init_client(&target).await?;
    let tools = client
        .list_tools()
        .await
        .map_err(|e| eprintln!("Warning: Failed to list tools from MCP server '{}': {e}", target.name))
        .ok()?;

    let filtered_tools: Vec<_> = tools
        .into_iter()
        .filter(|t| {
            is_tool_allowed(
                &t.name,
                target.config.include_tools.as_deref(),
                target.config.exclude_tools.as_deref(),
            )
        })
        .collect();

    let mode = target
        .config
        .mode
        .unwrap_or(rho_harness_core::config::McpExposureMode::Auto);

    Some(SingleServerLoaded {
        server_name: target.name.to_string(),
        client,
        tools: filtered_tools,
        max_bytes: target.max_bytes,
        mode,
    })
}

fn aggregate_loaded_servers(
    results: Vec<SingleServerLoaded>,
    config: &Config,
    active_history_tools: &std::collections::HashSet<String>,
) -> (Vec<DynamicTool>, super::search::DynamicToolActivator) {
    let activator = super::search::DynamicToolActivator::new();
    let mut initial_tools = Vec::new();
    let mut gateway_clients = std::collections::BTreeMap::new();
    let mut gateway_tool_defs = Vec::new();

    let mut non_gateway_servers = Vec::new();
    for loaded in results {
        if loaded.mode == rho_harness_core::config::McpExposureMode::Gateway {
            gateway_clients.insert(loaded.server_name.clone(), Arc::clone(&loaded.client));
            for tool in loaded.tools {
                gateway_tool_defs.push((loaded.server_name.clone(), tool));
            }
        } else {
            non_gateway_servers.push(loaded);
        }
    }

    if !gateway_clients.is_empty() {
        let gateway = super::gateway::McpGateway::new(gateway_clients, gateway_tool_defs, config.output_max_bytes);
        let (gw_tool, script_tool) = gateway.into_dynamic_tools();
        initial_tools.push(gw_tool);
        initial_tools.push(script_tool);
    }

    let total_tools: usize = non_gateway_servers.iter().map(|s| s.tools.len()).sum();
    let should_defer = total_tools > config.mcp.defer_threshold;

    let mut deferred_tools = Vec::new();
    for loaded in non_gateway_servers {
        for tool in loaded.tools {
            let wire_name = format!("{}_{}", loaded.server_name, tool.name);
            let must_expose_directly = loaded.mode == rho_harness_core::config::McpExposureMode::Direct
                || !should_defer
                || active_history_tools.contains(&wire_name);

            if must_expose_directly {
                activator.mark_activated(&wire_name);
                initial_tools.push(build_single_mcp_tool(
                    tool,
                    Arc::clone(&loaded.client),
                    (&loaded.server_name, loaded.max_bytes),
                ));
            } else {
                deferred_tools.push(super::search::DeferredMcpTool::new(
                    loaded.server_name.clone(),
                    tool.name,
                    tool.description.unwrap_or_default(),
                    tool.input_schema,
                    Arc::clone(&loaded.client),
                    loaded.max_bytes,
                ));
            }
        }
    }

    if !deferred_tools.is_empty() {
        let catalog = super::search::ToolSearchCatalog::new(deferred_tools);
        let search_tool = super::search::build_tool_search_tool(catalog, activator.clone());
        initial_tools.push(search_tool);
    }

    (initial_tools, activator)
}

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<DynamicTool> {
    let (tools, _) = load_mcp_tools_with_activator(config, working_dir, &std::collections::HashSet::new()).await;
    tools
}

pub async fn load_mcp_tools_with_activator(
    config: &Config,
    working_dir: &Path,
    active_history_tools: &std::collections::HashSet<String>,
) -> (Vec<DynamicTool>, super::search::DynamicToolActivator) {
    if !config.mcp.enabled {
        return (Vec::new(), super::search::DynamicToolActivator::new());
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

    let results: Vec<_> = futures::future::join_all(futures).await.into_iter().flatten().collect();
    aggregate_loaded_servers(results, config, active_history_tools)
}
