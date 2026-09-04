//! CLI subcommand execution (config, models, plugins, login, logout).

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::{Commands, PluginCommands};
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

pub async fn handle_command(
    cmd: Commands,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Login { provider } => {
            super::auth::login_provider(provider.as_deref(), config, auth_store).await?;
        }
        Commands::Logout { provider } => {
            super::auth::logout_provider(provider.as_deref(), config, auth_store)?;
        }
        Commands::Config { key, value } => {
            handle_config(key, value, config)?;
        }
        Commands::Models => {
            handle_models(config);
        }
        Commands::Plugin { action } => {
            handle_plugin(action, config);
        }
    }
    Ok(())
}

fn handle_config(
    key: Option<String>,
    value: Option<String>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    match (key, value) {
        (Some(k), Some(v)) => {
            Config::set_file_value(&config.config_dir, &k, &v)?;
            println!("Set {k} = {v} in {}", config.config_dir.join("config.toml").display());
        }
        (Some(_), None) | (None, Some(_)) => {
            println!("Usage: rho config <key> <value>");
        }
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
    Ok(())
}

fn handle_models(config: &Config) {
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
                ProviderId::Antigravity => {
                    for model in rho_engine::provider::discovery::antigravity_preset_models() {
                        println!("  - {} ({})", model.id, model.description);
                    }
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
}

fn handle_plugin(action: Option<PluginCommands>, config: &Config) {
    match action.unwrap_or(PluginCommands::List) {
        PluginCommands::List | PluginCommands::Inspect { .. } => {
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
        PluginCommands::Install { package, .. } => {
            println!(
                "To configure an MCP server or plugin, add it to config.toml under [mcp.servers.{package}] or [plugins.{package}]"
            );
        }
        PluginCommands::Remove { name: _ } => {
            println!("To remove an MCP server or plugin, remove it from config.toml");
        }
    }
}
