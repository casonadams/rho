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
        print_server_entry(name, server);
    }
}

fn format_server_transport(server: &McpServerConfig) -> &'static str {
    match server.resolved_transport() {
        McpTransportKind::Stdio => "stdio",
        McpTransportKind::StreamableHttp => "streamable-http",
        McpTransportKind::Sse => "sse",
    }
}

fn format_server_mode(server: &McpServerConfig) -> &'static str {
    match server.mode.unwrap_or(McpExposureMode::Auto) {
        McpExposureMode::Auto => "auto",
        McpExposureMode::Direct => "direct",
        McpExposureMode::Gateway => "gateway",
    }
}

fn print_server_entry(name: &str, server: &McpServerConfig) {
    let transport = format_server_transport(server);
    let mode = format_server_mode(server);
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

fn build_mcp_transport(server_cfg: &McpServerConfig) -> Result<Arc<McpTransport>, Box<dyn std::error::Error>> {
    let timeout = server_cfg.timeout_seconds.map(std::time::Duration::from_secs);
    if let Some(url) = &server_cfg.url {
        let kind = server_cfg.resolved_transport();
        Ok(McpTransport::new_http(url, kind, server_cfg.headers.clone(), timeout))
    } else {
        let working_dir = std::env::current_dir()?;
        let (stdin, stdout, handle) = McpProcess::spawn(server_cfg, &working_dir)?;
        Ok(McpTransport::new_stdio(stdin, stdout, handle, timeout))
    }
}

async fn probe_client_features(client: &McpClient) {
    probe_tools(client).await;
    probe_resources(client).await;
    probe_prompts(client).await;
}

async fn probe_tools(client: &McpClient) {
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
}

async fn probe_resources(client: &McpClient) {
    if let Ok(resources) = client.list_resources().await
        && !resources.is_empty()
    {
        println!("  ✓ Discovered {} resource(s):", resources.len());
        for r in &resources {
            println!("    - {} ({})", r.name, r.uri);
        }
    }
}

async fn probe_prompts(client: &McpClient) {
    if let Ok(prompts) = client.list_prompts().await
        && !prompts.is_empty()
    {
        println!("  ✓ Discovered {} prompt(s):", prompts.len());
        for p in &prompts {
            let desc = p.description.as_deref().unwrap_or("");
            println!("    - {}: {desc}", p.name);
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
    let transport = build_mcp_transport(server_cfg)?;
    let client = Arc::new(McpClient::new(name, transport));
    let init_res = client.initialize().await?;
    println!("  ✓ Initialized in {}ms", start.elapsed().as_millis());
    if let Some(server_info) = init_res.get("serverInfo") {
        println!("    Server info: {server_info}");
    }

    probe_client_features(&client).await;
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

fn build_server_config(target: &str, args: &[String]) -> McpServerConfig {
    if target.starts_with("http://") || target.starts_with("https://") {
        McpServerConfig {
            url: Some(target.to_string()),
            transport: Some(McpTransportKind::StreamableHttp),
            enabled: true,
            ..Default::default()
        }
    } else {
        McpServerConfig::stdio(target, args.to_vec())
    }
}

fn add_server(name: &str, target: &str, args: &[String], config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let server = build_server_config(target, args);
    Config::add_mcp_server(&config.config_dir, name, server)?;
    println!("✓ Added MCP server '{name}' to config.");
    Ok(())
}

fn remove_server(name: &str, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    Config::remove_mcp_server(&config.config_dir, name)?;
    println!("✓ Removed MCP server '{name}' from config.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_server_helpers() {
        let stdio = McpServerConfig {
            transport: Some(McpTransportKind::Stdio),
            ..Default::default()
        };
        assert_eq!(format_server_transport(&stdio), "stdio");

        let sse = McpServerConfig {
            transport: Some(McpTransportKind::Sse),
            ..Default::default()
        };
        assert_eq!(format_server_transport(&sse), "sse");

        let http = McpServerConfig {
            transport: Some(McpTransportKind::StreamableHttp),
            ..Default::default()
        };
        assert_eq!(format_server_transport(&http), "streamable-http");

        let auto = McpServerConfig {
            mode: Some(McpExposureMode::Auto),
            ..Default::default()
        };
        assert_eq!(format_server_mode(&auto), "auto");

        let direct = McpServerConfig {
            mode: Some(McpExposureMode::Direct),
            ..Default::default()
        };
        assert_eq!(format_server_mode(&direct), "direct");

        let gateway = McpServerConfig {
            mode: Some(McpExposureMode::Gateway),
            ..Default::default()
        };
        assert_eq!(format_server_mode(&gateway), "gateway");
    }

    #[test]
    fn test_list_servers_output() {
        let mut config = Config::default();
        list_servers(&config);

        config.mcp.servers.insert(
            "http_srv".to_string(),
            McpServerConfig {
                url: Some("http://localhost:8000/mcp".into()),
                enabled: true,
                args: vec!["--arg".into()],
                include_tools: Some(vec!["tool_a".into()]),
                exclude_tools: Some(vec!["tool_b".into()]),
                ..Default::default()
            },
        );
        config.mcp.servers.insert(
            "stdio_srv".to_string(),
            McpServerConfig {
                command: Some("echo".into()),
                enabled: false,
                ..Default::default()
            },
        );
        list_servers(&config);
    }

    #[tokio::test]
    async fn test_build_mcp_transport_http_and_stdio() {
        let http_cfg = McpServerConfig {
            url: Some("https://example.com/mcp".into()),
            timeout_seconds: Some(5),
            ..Default::default()
        };
        assert!(build_mcp_transport(&http_cfg).is_ok());

        let stdio_cfg = McpServerConfig::stdio("echo", vec!["test".into()]);
        assert!(build_mcp_transport(&stdio_cfg).is_ok());
    }

    #[tokio::test]
    async fn test_test_server_and_login_missing() {
        let config = Config::default();
        let mut auth_store = AuthStore::default();

        assert!(test_server("missing", &config).await.is_ok());
        assert!(login_server("missing", &config, &mut auth_store).await.is_ok());
    }

    #[test]
    fn test_build_server_config() {
        let http = build_server_config("https://example.com/mcp", &[]);
        assert_eq!(http.url, Some("https://example.com/mcp".into()));
        assert_eq!(http.transport, Some(McpTransportKind::StreamableHttp));
        assert!(http.enabled);

        let stdio = build_server_config("echo", &["hello".into()]);
        assert_eq!(stdio.command, Some("echo".into()));
        assert_eq!(stdio.args, vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn test_handle_mcp_actions() {
        let config = Config::default();
        let mut auth_store = AuthStore::default();

        handle_mcp(None, &config, &mut auth_store).await.unwrap();
        handle_mcp(Some(McpCommands::List), &config, &mut auth_store)
            .await
            .unwrap();
        handle_mcp(
            Some(McpCommands::Test { name: "missing".into() }),
            &config,
            &mut auth_store,
        )
        .await
        .unwrap();
        handle_mcp(
            Some(McpCommands::Login { name: "missing".into() }),
            &config,
            &mut auth_store,
        )
        .await
        .unwrap();
    }
}
