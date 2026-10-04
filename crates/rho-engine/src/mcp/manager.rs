use super::cache::{McpCache, compute_definition_hash};
use super::client::McpToolDefinition;
use super::pool::McpServerPool;
use super::trust::is_server_definition_trusted;
use rho_harness_core::config::{Config, McpDirectTools, McpExposureMode, McpLifecycleMode, McpServerConfig};
use rig::tool::{DynamicTool, ToolOutput};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct DiscoveredServer {
    pub server_name: String,
    pub tools: Vec<McpToolDefinition>,
    pub max_bytes: usize,
    pub config: McpServerConfig,
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

fn build_pooled_mcp_tool(
    tool: McpToolDefinition,
    pool: McpServerPool,
    server_name: &str,
    max_bytes: usize,
) -> DynamicTool {
    let tool_name = format!("{}_{}", server_name, tool.name);
    let description = format!("[MCP: {}] {}", server_name, tool.description.unwrap_or_default());
    let mut schema = tool.input_schema;
    crate::tools::normalize_schema(&mut schema);
    let original_name = tool.name;
    let s_name = server_name.to_string();

    DynamicTool::new(tool_name, description, schema, move |args| {
        let pool = pool.clone();
        let original_name = original_name.clone();
        let server_name = s_name.clone();
        Box::pin(async move {
            match pool.get_or_connect(&server_name).await {
                Ok((client, in_flight)) => {
                    let res = client.call_tool(&original_name, args).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    pool.touch_activity(&server_name).await;
                    match res {
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
                }
                Err(e) => Ok(ToolOutput::text(format!(
                    "[MCP Error] Failed to connect to server '{server_name}': {e}"
                ))),
            }
        })
    })
}

async fn discover_uncached_tools(
    pool: &McpServerPool,
    name: &str,
    lifecycle: McpLifecycleMode,
) -> Option<Vec<McpToolDefinition>> {
    let (client, in_flight) = pool
        .get_or_connect(name)
        .await
        .map_err(|e| eprintln!("Warning: Failed to connect to MCP server '{name}': {e}"))
        .ok()?;

    let list_res = client
        .list_tools()
        .await
        .map_err(|e| eprintln!("Warning: Failed to list tools from MCP server '{name}': {e}"))
        .ok();

    in_flight.fetch_sub(1, Ordering::SeqCst);

    if lifecycle == McpLifecycleMode::Lazy {
        pool.disconnect(name).await;
    }

    list_res
}

async fn discover_server_tools(
    name: &str,
    cfg: &McpServerConfig,
    pool: &McpServerPool,
    cache: &mut McpCache,
    working_dir: &Path,
    config: &Config,
) -> Option<DiscoveredServer> {
    let def_hash = compute_definition_hash(cfg);
    let is_project_scoped =
        working_dir.join(".mcp.json").is_file() || working_dir.join(".agents").join("mcp.json").is_file();

    if is_project_scoped
        && !is_server_definition_trusted(working_dir, name, &def_hash, &config.config_dir)
        && !std::env::var("RHO_TRUST_PROJECT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    {
        eprintln!("Warning: Skipping untrusted project MCP server '{name}'. Definition has not been approved.");
        return None;
    }

    let lifecycle = cfg.lifecycle.unwrap_or(McpLifecycleMode::Lazy);
    let tools = match cache.get_valid_tools(name, &def_hash) {
        Some(cached) => cached.to_vec(),
        None => {
            let tools = discover_uncached_tools(pool, name, lifecycle).await?;
            cache.update(name, def_hash, tools.clone(), None);
            let _ = cache.save_to_dir(&config.config_dir);
            tools
        }
    };

    let filtered: Vec<_> = tools
        .into_iter()
        .filter(|t| is_tool_allowed(&t.name, cfg.include_tools.as_deref(), cfg.exclude_tools.as_deref()))
        .collect();

    Some(DiscoveredServer {
        server_name: name.to_string(),
        tools: filtered,
        max_bytes: config.output_max_bytes,
        config: cfg.clone(),
    })
}

fn is_direct_tool(
    name: &str,
    wire_name: &str,
    direct_tools: Option<&McpDirectTools>,
    mode: McpExposureMode,
    should_defer: bool,
    history: &HashSet<String>,
) -> bool {
    if history.contains(wire_name) {
        return true;
    }
    match direct_tools {
        Some(McpDirectTools::All(true)) => true,
        Some(McpDirectTools::All(false)) => false,
        Some(McpDirectTools::Search(_)) => false,
        Some(McpDirectTools::List(list)) => list.iter().any(|t| t == name || t == wire_name),
        None => mode == McpExposureMode::Direct || !should_defer,
    }
}

struct ToolRoutingContext<'a> {
    should_defer: bool,
    active_history_tools: &'a HashSet<String>,
    activator: &'a super::search::DynamicToolActivator,
    pool: &'a McpServerPool,
}

fn route_discovered_server(
    server: DiscoveredServer,
    ctx: &ToolRoutingContext<'_>,
    initial_tools: &mut Vec<DynamicTool>,
    deferred_tools: &mut Vec<super::search::DeferredMcpTool>,
) {
    let mode = server.config.mode.unwrap_or(McpExposureMode::Auto);
    let DiscoveredServer {
        server_name,
        tools,
        max_bytes,
        config,
    } = server;

    for tool in tools {
        let wire_name = format!("{server_name}_{}", tool.name);
        let direct = is_direct_tool(
            &tool.name,
            &wire_name,
            config.direct_tools.as_ref(),
            mode,
            ctx.should_defer,
            ctx.active_history_tools,
        );

        if direct {
            ctx.activator.mark_activated(&wire_name);
            initial_tools.push(build_pooled_mcp_tool(tool, ctx.pool.clone(), &server_name, max_bytes));
        } else {
            let mut keywords = Vec::new();
            if let Some(list) = config.search_keywords.get("*") {
                keywords.extend(list.clone());
            }
            if let Some(list) = config.search_keywords.get(&tool.name) {
                keywords.extend(list.clone());
            }
            deferred_tools.push(super::search::DeferredMcpTool::from_definition(
                server_name.clone(),
                tool,
                ctx.pool.clone(),
                max_bytes,
                keywords,
            ));
        }
    }
}

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<DynamicTool> {
    let (tools, _) = load_mcp_tools_with_activator(config, working_dir, &HashSet::new()).await;
    tools
}

pub async fn load_mcp_tools_with_activator(
    config: &Config,
    working_dir: &Path,
    active_history_tools: &HashSet<String>,
) -> (Vec<DynamicTool>, super::search::DynamicToolActivator) {
    if !config.mcp.enabled {
        return (Vec::new(), super::search::DynamicToolActivator::new());
    }

    let pool = McpServerPool::new(config.mcp.idle_timeout_seconds);
    pool.clone().spawn_reaper_task(Duration::from_secs(30));

    for (name, cfg) in &config.mcp.servers {
        if cfg.enabled {
            pool.register(name, cfg.clone(), working_dir).await;
        }
    }

    let mut cache = McpCache::load_from_dir(&config.config_dir);
    let mut discovered = Vec::new();

    for (name, cfg) in &config.mcp.servers {
        if cfg.enabled
            && let Some(d) = discover_server_tools(name, cfg, &pool, &mut cache, working_dir, config).await
        {
            if cfg.lifecycle == Some(McpLifecycleMode::Eager) {
                let _ = pool.get_or_connect(name).await;
            }
            discovered.push(d);
        }
    }

    let activator = super::search::DynamicToolActivator::new();
    let mut initial_tools = Vec::new();
    let total_tools: usize = discovered.iter().map(|s| s.tools.len()).sum();
    let should_defer = total_tools > config.mcp.defer_threshold;

    let ctx = ToolRoutingContext {
        should_defer,
        active_history_tools,
        activator: &activator,
        pool: &pool,
    };

    let mut deferred_tools = Vec::new();
    for srv in discovered {
        route_discovered_server(srv, &ctx, &mut initial_tools, &mut deferred_tools);
    }

    if !deferred_tools.is_empty() {
        let catalog = super::search::ToolSearchCatalog::new(deferred_tools);
        let search_tool = super::search::build_tool_search_tool(catalog, activator.clone());
        initial_tools.push(search_tool);
    }

    (initial_tools, activator)
}
