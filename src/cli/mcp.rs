use crate::config::Config;
use crate::config::cli::McpCommands;
use rho_engine::mcp::oauth::{LoginRequest, McpTokenStore};
use rho_engine::mcp::pool::build_transport;
use rho_engine::mcp::{
    McpClient, McpDiagnostic, McpTransport, approve_server_definition, compute_definition_hash, project_server_approval,
};
use rho_harness_core::config::{McpExposureMode, McpServerConfig, McpTransportKind};
use std::sync::Arc;
use std::time::Instant;

pub async fn handle_mcp(action: Option<McpCommands>, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    match action.unwrap_or(McpCommands::List) {
        McpCommands::List => {
            list_servers(config);
            Ok(())
        }
        McpCommands::Test { name } => test_server(&name, config).await,
        McpCommands::Add { name, target, args } => add_server(&name, &target, &args, config),
        McpCommands::Remove { name } => remove_server(&name, config),
        McpCommands::Login { name } => Ok(login_mcp_server(&name, config).await?),
        McpCommands::Logout { name } => Ok(logout_mcp_server(&name, config)?),
        McpCommands::Approve { names } => Ok(approve_project_servers(&names, config)?),
    }
}

fn token_store(config: &Config) -> McpTokenStore {
    McpTokenStore::in_config_dir(&config.config_dir)
}

fn servers_where(diagnostics: &[McpDiagnostic], pick: fn(&McpDiagnostic) -> bool) -> Vec<&str> {
    diagnostics
        .iter()
        .filter(|d| pick(d))
        .map(McpDiagnostic::server)
        .collect()
}

pub fn mcp_diagnostic_lines(diagnostics: &[McpDiagnostic]) -> Vec<String> {
    let mut lines = Vec::new();
    let needs_login = servers_where(diagnostics, |d| matches!(d, McpDiagnostic::NeedsLogin { .. }));
    if !needs_login.is_empty() {
        lines.push(format!(
            "MCP sign-in needed: {} (run /mcp login <server>)",
            needs_login.join(", ")
        ));
    }
    let pending = servers_where(diagnostics, |d| matches!(d, McpDiagnostic::Pending { .. }));
    if !pending.is_empty() {
        lines.push(format!(
            "MCP still connecting: {} (their tools join tool_search when ready)",
            pending.join(", ")
        ));
    }
    let unapproved = servers_where(diagnostics, |d| matches!(d, McpDiagnostic::Untrusted { .. }));
    if !unapproved.is_empty() {
        lines.push(format!(
            "MCP project servers skipped until approved: {} (review them, then run rho mcp approve <server>)",
            unapproved.join(", ")
        ));
    }
    for diagnostic in diagnostics {
        match diagnostic {
            McpDiagnostic::NeedsLogin { .. } | McpDiagnostic::Pending { .. } | McpDiagnostic::Untrusted { .. } => {}
            McpDiagnostic::Failed { server, message } => {
                lines.push(format!("MCP server '{server}' unavailable: {message}"));
            }
            McpDiagnostic::NameConflict { server, .. } => {
                lines.push(format!("MCP server '{server}': {}", diagnostic.message()));
            }
        }
    }
    lines
}

pub fn eprint_mcp_diagnostics(engine: &rho_engine::engine::AgentEngine) {
    for line in mcp_diagnostic_lines(engine.mcp_diagnostics()) {
        eprintln!("Warning: {line}");
    }
    let Some(runtime) = engine.mcp_runtime() else {
        return;
    };
    let runtime = std::sync::Arc::downgrade(runtime);
    tokio::spawn(async move {
        while let Some(runtime) = runtime.upgrade() {
            let ready = tokio::time::timeout(std::time::Duration::from_secs(5), runtime.problems_ready()).await;
            if ready.is_ok() {
                for line in mcp_diagnostic_lines(&runtime.take_problems()) {
                    eprintln!("Warning: {line}");
                }
            }
        }
    });
}

pub async fn login_mcp_server(name: &str, config: &Config) -> crate::error::Result<()> {
    login_mcp_server_with(name, config, &crate::cli::auth::TerminalOAuthCallbacks).await?;
    println!("Signed in to MCP server '{name}'.");
    Ok(())
}

pub async fn login_mcp_server_with(
    name: &str,
    config: &Config,
    callbacks: &dyn rho_harness_core::auth::OAuthLoginCallbacks,
) -> crate::error::Result<()> {
    let server = config
        .mcp
        .servers
        .get(name)
        .ok_or_else(|| crate::error::AppError::Config(format!("MCP server '{name}' is not configured")))?;
    let url = server.url.as_deref().ok_or_else(|| {
        crate::error::AppError::Config(format!(
            "MCP server '{name}' runs locally over stdio; only remote (URL) servers use OAuth"
        ))
    })?;
    if !server.uses_oauth() {
        callbacks
            .on_progress(&format!(
                "Note: '{name}' configures headers and no `oauth` block, so rho sends those headers instead of OAuth tokens."
            ))
            .await?;
    }
    let request = LoginRequest {
        server: name,
        url,
        config: server.oauth.as_ref(),
    };
    rho_engine::mcp::oauth::login(request, &token_store(config), callbacks).await
}

pub fn logout_mcp_server(name: &str, config: &Config) -> crate::error::Result<()> {
    if token_store(config).forget(name)? {
        println!("Removed stored OAuth tokens for MCP server '{name}'.");
    } else {
        println!("No stored OAuth tokens for MCP server '{name}'.");
    }
    Ok(())
}

fn approve_project_servers(names: &[String], config: &Config) -> crate::error::Result<()> {
    let project = config.mcp.project.as_ref().ok_or_else(|| {
        crate::error::AppError::Config(
            "No project MCP servers here; only servers from .agents/mcp.json, .claude/mcp.json, or .mcp.json in the current directory need approval".into(),
        )
    })?;
    let selected: Vec<&String> = if names.is_empty() {
        project.names.iter().collect()
    } else {
        names.iter().collect()
    };
    let mut approvals = Vec::with_capacity(selected.len());
    for name in selected {
        match config.mcp.servers.get(name) {
            Some(server) if project.names.contains(name) => approvals.push((name, server)),
            Some(_) => {
                return Err(crate::error::AppError::Config(format!(
                    "MCP server '{name}' is not defined by this project, so it needs no approval"
                )));
            }
            None => {
                return Err(crate::error::AppError::Config(format!(
                    "MCP server '{name}' is not configured"
                )));
            }
        }
    }
    for (name, server) in approvals {
        let hash = compute_definition_hash(server);
        approve_server_definition(&project.workspace, name, &hash, &config.config_dir)?;
        println!(
            "Approved project MCP server '{name}' ({}) for {}",
            server_target(server),
            project.workspace.display()
        );
    }
    Ok(())
}

fn list_servers(config: &Config) {
    println!("Configured MCP Servers ({} total):", config.mcp.servers.len());
    if config.mcp.servers.is_empty() {
        println!("  (none configured)");
        return;
    }

    for (name, server) in &config.mcp.servers {
        print_server_entry(name, server, &token_store(config));
        match project_server_approval(config, name, server) {
            Some(true) => println!("      project: approved"),
            Some(false) => println!("      project: not approved (run rho mcp approve {name})"),
            None => {}
        }
    }
}

fn server_target(server: &McpServerConfig) -> &str {
    server
        .command
        .as_deref()
        .or(server.url.as_deref())
        .unwrap_or("<unspecified>")
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

fn print_server_entry(name: &str, server: &McpServerConfig, tokens: &McpTokenStore) {
    let transport = format_server_transport(server);
    let mode = format_server_mode(server);
    let target = server_target(server);
    let status = if server.enabled { "enabled" } else { "disabled" };

    println!("  - {name:16} [{transport}] mode={mode} ({status}) target='{target}'");
    if server.uses_oauth() {
        let auth = if tokens.is_signed_in(name) {
            "signed in"
        } else {
            "not signed in"
        };
        println!("      oauth: {auth}");
    }
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

async fn build_mcp_transport(
    name: &str,
    server_cfg: &McpServerConfig,
    config: &Config,
) -> Result<Arc<McpTransport>, Box<dyn std::error::Error>> {
    let working_dir = std::env::current_dir()?;
    Ok(build_transport(name, server_cfg, &working_dir, Some(&token_store(config))).await?)
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
    let transport = build_mcp_transport(name, server_cfg, config).await?;
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
    use rho_harness_core::config::ProjectMcpServers;

    #[test]
    fn diagnostics_group_sign_ins_into_one_line() {
        let server = |s: &str| s.to_string();
        let diagnostics = [
            McpDiagnostic::NeedsLogin {
                server: server("figma"),
            },
            McpDiagnostic::Failed {
                server: server("broken"),
                message: "spawn failed".into(),
            },
            McpDiagnostic::NeedsLogin {
                server: server("linear"),
            },
            McpDiagnostic::Untrusted { server: server("proj") },
            McpDiagnostic::Pending { server: server("slow") },
            McpDiagnostic::NameConflict {
                server: server("dup"),
                tool: "read".into(),
                wire_name: "dup_read".into(),
            },
        ];
        assert_eq!(
            mcp_diagnostic_lines(&diagnostics),
            [
                "MCP sign-in needed: figma, linear (run /mcp login <server>)",
                "MCP still connecting: slow (their tools join tool_search when ready)",
                "MCP project servers skipped until approved: proj (review them, then run rho mcp approve <server>)",
                "MCP server 'broken' unavailable: spawn failed",
                "MCP server 'dup': tool 'read' kept behind tool_search: name 'dup_read' is already taken",
            ]
        );
        assert!(mcp_diagnostic_lines(&[]).is_empty());
    }
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
        let config = Config::default();
        let http_cfg = McpServerConfig {
            url: Some("https://example.com/mcp".into()),
            timeout_seconds: Some(5),
            ..Default::default()
        };
        assert!(build_mcp_transport("h", &http_cfg, &config).await.is_ok());

        let stdio_cfg = McpServerConfig::stdio("echo", vec!["test".into()]);
        assert!(build_mcp_transport("s", &stdio_cfg, &config).await.is_ok());
    }

    #[tokio::test]
    async fn login_and_logout_explain_unusable_targets() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config {
            config_dir: dir.path().to_path_buf(),
            ..Config::default()
        };
        config
            .mcp
            .servers
            .insert("local".into(), McpServerConfig::stdio("echo", vec![]));
        let missing = login_mcp_server("ghost", &config).await.unwrap_err();
        assert!(missing.to_string().contains("'ghost' is not configured"), "{missing}");
        let stdio = login_mcp_server("local", &config).await.unwrap_err();
        assert!(
            stdio.to_string().contains("only remote (URL) servers use OAuth"),
            "{stdio}"
        );
        logout_mcp_server("local", &config).unwrap();
    }

    #[tokio::test]
    async fn test_handle_mcp_actions() {
        let config = Config::default();

        handle_mcp(None, &config).await.unwrap();
        handle_mcp(Some(McpCommands::List), &config).await.unwrap();
        handle_mcp(Some(McpCommands::Test { name: "missing".into() }), &config)
            .await
            .unwrap();
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

    fn workspace_config(dir: &std::path::Path) -> Config {
        let mut config = Config {
            config_dir: dir.join("config"),
            ..Config::default()
        };
        for name in ["global", "proj_a", "proj_b"] {
            config
                .mcp
                .servers
                .insert(name.into(), McpServerConfig::stdio(format!("{name}-cmd"), vec![]));
        }
        config.mcp.project = Some(ProjectMcpServers {
            workspace: dir.to_path_buf(),
            names: ["proj_a".to_string(), "proj_b".to_string()].into(),
        });
        config
    }

    fn approval(config: &Config, name: &str) -> Option<bool> {
        project_server_approval(config, name, &config.mcp.servers[name])
    }

    #[test]
    fn approve_marks_named_project_servers_approved() {
        let dir = tempfile::tempdir().unwrap();
        let config = workspace_config(dir.path());

        approve_project_servers(&["proj_a".into()], &config).unwrap();

        assert_eq!(approval(&config, "proj_a"), Some(true));
        assert_eq!(approval(&config, "proj_b"), Some(false));
        assert_eq!(approval(&config, "global"), None);
    }

    #[test]
    fn approve_without_names_approves_every_project_server() {
        let dir = tempfile::tempdir().unwrap();
        let config = workspace_config(dir.path());

        approve_project_servers(&[], &config).unwrap();

        assert_eq!(approval(&config, "proj_a"), Some(true));
        assert_eq!(approval(&config, "proj_b"), Some(true));
    }

    #[test]
    fn approve_rejects_servers_outside_the_project_without_approving_any() {
        let dir = tempfile::tempdir().unwrap();
        let config = workspace_config(dir.path());
        let cases = [
            ("global", "is not defined by this project"),
            ("ghost", "is not configured"),
        ];
        for (name, expected) in cases {
            let err = approve_project_servers(&["proj_a".into(), name.into()], &config).unwrap_err();
            assert!(err.to_string().contains(expected), "{name}: {err}");
            assert_eq!(approval(&config, "proj_a"), Some(false), "{name}");
        }

        let no_project = Config {
            config_dir: dir.path().join("config"),
            ..Config::default()
        };
        let err = approve_project_servers(&[], &no_project).unwrap_err();
        assert!(err.to_string().contains("No project MCP servers here"), "{err}");
    }
}
