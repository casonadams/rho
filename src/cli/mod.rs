pub mod auth;
pub mod rpc;

#[cfg(test)]
mod tests;

pub use auth::{login_provider, logout_provider};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::{Cli, Commands};
#[cfg(feature = "ui")]
use crate::repl::ReplSession;
#[cfg(feature = "ui")]
use crate::ui::TerminalRenderer;
use rho_harness_core::provider::ProviderId;
use rho_harness_core::session::SessionManager;
use std::io::Read;
use std::str::FromStr;

pub async fn run_cli() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = <Cli as clap::Parser>::parse();
    let config = Config::load(Some(&cli))?;
    // Retained so /reload can re-apply CLI overrides after re-reading config.
    let cli_for_repl = cli.clone();
    config.ensure_dirs()?;

    let mut auth_store = AuthStore::load(&config.auth_file)?;

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Login { provider } => {
                login_provider(provider.as_deref(), &config, &mut auth_store).await?;
                return Ok(());
            }
            Commands::Logout { provider } => {
                logout_provider(provider.as_deref(), &config, &mut auth_store)?;
                return Ok(());
            }
            Commands::Config { key, value } => {
                match (key, value) {
                    (Some(k), Some(v)) => {
                        Config::set_file_value(&config.config_dir, &k, &v)?;
                        println!("Set {k} = {v} in {}", config.config_dir.join("config.toml").display());
                    }
                    (Some(_), None) => println!("Usage: rho config <key> <value>"),
                    (None, Some(_)) => println!("Usage: rho config <key> <value>"),
                    (None, None) => {
                        println!("Config location: {}", config.config_dir.display());
                        match ProviderId::from_str(&config.provider) {
                            Ok(provider) => {
                                println!("Model: {}", config.model);
                                println!("Provider: {provider} ({})", provider.auth_mode_label());
                            }
                            Err(_) => {
                                println!("Model: {}", config.model);
                                println!("Provider: {} (custom)", config.provider);
                            }
                        }
                        println!("Auto approve: {}", config.auto_approve);
                        println!("Max turns: {}", config.max_turns);
                        println!("Context window messages: {}", config.context_window_messages);
                        println!("Compaction max bytes: {}", config.compaction_max_bytes);
                    }
                }
                return Ok(());
            }
            Commands::Models => {
                match ProviderId::from_str(&config.provider) {
                    Ok(provider) => {
                        println!("Models for {provider}:");
                        match provider {
                            ProviderId::Anthropic => {
                                println!(
                                    "  - claude-3-7-sonnet-20250219\n  - claude-3-5-sonnet-20241022\n  - claude-3-5-haiku-20241022"
                                );
                            }
                            ProviderId::OpenAi => {
                                println!("  - gpt-4o\n  - gpt-4o-mini\n  - o1\n  - o3-mini");
                            }
                            ProviderId::Gemini => {
                                println!("  - gemini-2.0-flash\n  - gemini-1.5-pro\n  - gemini-1.5-flash");
                            }
                            ProviderId::DeepSeek => {
                                println!("  - deepseek-chat\n  - deepseek-reasoner");
                            }
                            _ => {
                                println!("  - {}", config.model);
                            }
                        }
                    }
                    Err(_) => {
                        println!("Models for {} (custom):", config.provider);
                        println!("  - {}", config.model);
                    }
                }
                return Ok(());
            }
            Commands::Plugin { action } => {
                match action.unwrap_or(crate::config::cli::PluginCommands::List) {
                    crate::config::cli::PluginCommands::List | crate::config::cli::PluginCommands::Inspect { .. } => {
                        println!("Configured MCP Servers & Plugins:");
                        if config.mcp.servers.is_empty() && config.plugins.is_empty() {
                            println!("  (none configured)");
                        } else {
                            for (name, server) in &config.mcp.servers {
                                println!(
                                    "  - [mcp] {name}: command='{}' enabled={}",
                                    server.command, server.enabled
                                );
                            }
                            for (name, plugin) in &config.plugins {
                                let target = plugin
                                    .command
                                    .as_deref()
                                    .map(|c| c.to_string())
                                    .unwrap_or_else(|| plugin.path.display().to_string());
                                println!("  - [plugin] {name}: target='{target}' enabled={}", plugin.enabled);
                            }
                        }
                    }
                    crate::config::cli::PluginCommands::Install { package, .. } => {
                        println!(
                            "To configure an MCP server or plugin, add it to config.toml under [mcp.servers.{package}] or [plugins.{package}]"
                        );
                    }
                    crate::config::cli::PluginCommands::Remove { name: _ } => {
                        println!("To remove an MCP server or plugin, remove it from config.toml");
                    }
                }
                return Ok(());
            }
        }
    }

    let prompt_text = if let Some(p) = cli.prompt {
        Some(p)
    } else if !cli.message.is_empty() {
        Some(cli.message.join(" "))
    } else if !atty_check() {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer).ok();
        let trimmed = buffer.trim().to_string();
        if !trimmed.is_empty() { Some(trimmed) } else { None }
    } else {
        None
    };

    let resume_target = if cli.resume_picker {
        crate::ui::interactive::session_picker::prompt_session_picker(&config.sessions_dir)?
    } else if cli.r#continue {
        let cwd = std::env::current_dir()?;
        SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?
    } else {
        cli.resume
    };

    if let Some(export_path) = cli.export {
        let resume_target_id = match resume_target {
            Some(id) => id,
            None => {
                let cwd = std::env::current_dir()?;
                SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?.ok_or_else(|| {
                    rho_harness_core::error::AppError::Session("no session found to export".to_string())
                })?
            }
        };
        let session_manager = SessionManager::new(&config.sessions_dir, Some(&resume_target_id))?;
        let tree = session_manager.load_tree().await?;
        let export_path = std::path::PathBuf::from(export_path);
        let content = if export_path.extension().and_then(|ext| ext.to_str()) == Some("html") {
            rho_harness_core::session::export::render_html(&tree, &resume_target_id)
        } else {
            rho_harness_core::session::export::render_markdown(&tree, &resume_target_id)
        };
        if let Some(parent) = export_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&export_path, content)?;
        println!("Exported session {} to {}", resume_target_id, export_path.display());
        return Ok(());
    }

    if cli.mode == "rpc" {
        return rpc::run_rpc_daemon(config, auth_store).await.map_err(Into::into);
    }

    if cli.mode == "json" {
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<rho_harness_core::rpc::protocol::RpcEvent>();
        let (presenter, _) = crate::ui::render::RpcPresenter::new(event_tx);
        let presenter_arc: std::sync::Arc<dyn rho_harness_core::presentation::Presenter> =
            std::sync::Arc::new(presenter);

        let writer_task = tokio::spawn(async move {
            let mut writer = rho_harness_core::rpc::transport::JsonLinesWriter::new(tokio::io::stdout());
            while let Some(event) = event_rx.recv().await {
                let _ = writer.write_message(&event).await;
            }
        });

        if let Some(prompt) = prompt_text {
            let engine = crate::platform::agent_engine(config, auth_store, resume_target.as_deref()).await?;
            let res = engine
                .run_turn(crate::engine::runner::TurnRequest::new(&prompt), presenter_arc.clone())
                .await;
            drop(presenter_arc);
            let _ = writer_task.await;
            return match res {
                Ok(_) => Ok(()),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
        }
    }

    if let Some(prompt) = prompt_text {
        let engine = crate::platform::agent_engine(config, auth_store, resume_target.as_deref()).await?;
        if let Some(ref name) = cli.name {
            let _ = engine.session_manager.set_session_name(name).await;
        }
        #[cfg(feature = "ui")]
        let presenter: std::sync::Arc<dyn rho_harness_core::presentation::Presenter> =
            std::sync::Arc::new(TerminalRenderer::default());
        #[cfg(not(feature = "ui"))]
        let presenter: std::sync::Arc<dyn rho_harness_core::presentation::Presenter> =
            std::sync::Arc::new(rho_harness_core::presentation::StructuredPresenter::stdout());

        let res = engine
            .run_turn(crate::engine::runner::TurnRequest::new(&prompt), presenter.clone())
            .await;
        presenter.flush();

        #[cfg(feature = "ui")]
        println!();
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        #[cfg(feature = "ui")]
        {
            let mut session = ReplSession::new(config, auth_store, resume_target).with_cli(Some(cli_for_repl));
            session.run().await?;
            Ok(())
        }
        #[cfg(not(feature = "ui"))]
        {
            Err(Box::new(crate::error::AppError::Session(
                "interactive REPL is unavailable in headless mode (compiled without 'ui' feature); provide a prompt via -p or piped stdin".to_string(),
            )))
        }
    }
}

fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}
