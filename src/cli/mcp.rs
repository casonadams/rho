use crate::auth::AuthStore;
use crate::cli::auth::TerminalOAuthCallbacks;
use crate::config::Config;
use crate::config::cli::McpCommands;
use rho_engine::mcp::{McpClient, McpProcess, McpTransport};
use rho_harness_core::config::{McpExposureMode, McpServerConfig, McpTransportKind};
use std::sync::Arc;
use std::time::Instant;

pub async fn handle_mcp(
    action: Option<McpCommands>,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    match action.unwrap_or(McpCommands::List) {
        McpCommands::List => {
            list_servers(config);
            Ok(())
        }
        McpCommands::Test { name } => test_server(&name, config).await,
        McpCommands::Login { name } => login_server(&name, config, auth_store).await,
        McpCommands::Add { name, target, args } => add_server(&name, &target, &args, config),
        McpCommands::Remove { name } => remove_server(&name, config),
    }
}

fn list_servers(config: &Config) {
    println!("Configured MCP Servers ({} total):", config.mcp.servers.len());
    if config.mcp.servers.is_empty() {
        println!("  (none configured)");
        return;
    }

    for (name, server) in &config.mcp.servers {
        let transport = match server.resolved_transport() {
            McpTransportKind::Stdio => "stdio",
            McpTransportKind::StreamableHttp => "streamable-http",
            McpTransportKind::Sse => "sse",
        };
        let mode = match server.mode.unwrap_or(McpExposureMode::Auto) {
            McpExposureMode::Auto => "auto",
            McpExposureMode::Direct => "direct",
            McpExposureMode::Gateway => "gateway",
        };
        let target = server
            .command
            .as_deref()
            .or(server.url.as_deref())
            .unwrap_or("<unspecified>");
        let status = if server.enabled { "enabled" } else { "disabled" };

        println!("  - {name:16} [{transport}] mode={mode} ({status}) target='{target}'");
        if !server.args.is_empty() {
            println!("      args: {:?}", server.args);
        }
        if let Some(inc) = &server.include_tools {
            println!("      include_tools: {:?}", inc);
        }
        if let Some(exc) = &server.exclude_tools {
            println!("      exclude_tools: {:?}", exc);
        }
    }
}

async fn test_server(name: &str, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let Some(server_cfg) = config.mcp.servers.get(name) else {
        eprintln!("Error: MCP server '{name}' not found in configuration.");
        return Ok(());
    };

    println!("Testing MCP server '{name}'...");
    let start = Instant::now();

    let timeout = server_cfg.timeout_seconds.map(std::time::Duration::from_secs);
    let transport = if let Some(url) = &server_cfg.url {
        let kind = server_cfg.resolved_transport();
        McpTransport::new_http(url, kind, server_cfg.headers.clone(), timeout)
    } else {
        let working_dir = std::env::current_dir()?;
        let (stdin, stdout, handle) = McpProcess::spawn(server_cfg, &working_dir)?;
        McpTransport::new_stdio(stdin, stdout, handle, timeout)
    };

    let client = Arc::new(McpClient::new(name, transport));
    let init_res = client.initialize().await?;
    let elapsed = start.elapsed();
    println!("  ✓ Initialized in {}ms", elapsed.as_millis());
    if let Some(server_info) = init_res.get("serverInfo") {
        println!("    Server info: {server_info}");
    }

    match client.list_tools().await {
        Ok(tools) => {
            println!("  ✓ Discovered {} tool(s):", tools.len());
            for t in &tools {
                let desc = t.description.as_deref().unwrap_or("");
                println!("    - {}: {desc}", t.name);
            }
        }
        Err(e) => println!("    Tools discovery: failed ({e})"),
    }

    match client.list_resources().await {
        Ok(resources) if !resources.is_empty() => {
            println!("  ✓ Discovered {} resource(s):", resources.len());
            for r in &resources {
                println!("    - {} ({})", r.name, r.uri);
            }
        }
        _ => {}
    }

    match client.list_prompts().await {
        Ok(prompts) if !prompts.is_empty() => {
            println!("  ✓ Discovered {} prompt(s):", prompts.len());
            for p in &prompts {
                let desc = p.description.as_deref().unwrap_or("");
                println!("    - {}: {desc}", p.name);
            }
        }
        _ => {}
    }

    println!("Server '{name}' is operational.");
    Ok(())
}

async fn login_server(
    name: &str,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(server_cfg) = config.mcp.servers.get(name) else {
        eprintln!("Error: MCP server '{name}' not found in configuration.");
        return Ok(());
    };

    println!("Starting OAuth 2.1 login for MCP server '{name}'...");
    let callbacks = TerminalOAuthCallbacks;
    let cred = rho_engine::mcp::login_mcp_server(name, server_cfg, auth_store, &callbacks).await?;
    println!("✓ Successfully authenticated MCP server '{name}'. Token stored in auth store.");
    let _ = cred;
    Ok(())
}

fn add_server(name: &str, target: &str, args: &[String], config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let is_url = target.starts_with("http://") || target.starts_with("https://");
    let server = if is_url {
        McpServerConfig {
            url: Some(target.to_string()),
            transport: Some(McpTransportKind::StreamableHttp),
            enabled: true,
            ..Default::default()
        }
    } else {
        McpServerConfig::stdio(target, args.to_vec())
    };

    Config::add_mcp_server(&config.config_dir, name, server)?;
    println!("✓ Added MCP server '{name}' to config.");
    Ok(())
}

fn remove_server(name: &str, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    Config::remove_mcp_server(&config.config_dir, name)?;
    println!("✓ Removed MCP server '{name}' from config.");
    Ok(())
}
