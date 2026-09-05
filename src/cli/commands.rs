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
            handle_config(key, value, config).await?;
        }
        Commands::Models => {
            handle_models(config);
        }
        Commands::Install { target, force } => {
            handle_plugin(Some(PluginCommands::Install { target, force }), config).await?;
        }
        Commands::Update { target } => match target.as_deref() {
            None => {
                super::plugin::self_update::handle_self_update(config).await?;
            }
            Some("all") => {
                super::plugin::update::handle_update_all(config).await?;
            }
            Some(plugin) => {
                super::plugin::update::handle_update_plugin(config, plugin).await?;
            }
        },
        Commands::Remove { name, keep_binary } => {
            handle_plugin(Some(PluginCommands::Remove { name, keep_binary }), config).await?;
        }
        Commands::Plugin { action } => {
            handle_plugin(action, config).await?;
        }
    }
    Ok(())
}

async fn handle_config(
    key: Option<String>,
    value: Option<String>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    match (key, value) {
        (Some(k), Some(v)) => {
            Config::set_file_value_async(&config.config_dir, &k, &v).await?;
            println!("Set {k} = {v} in {}", config.config_dir.join("config.toml").display());
        }
        (Some(_), None) | (None, Some(_)) => {
            println!("Usage: rho config <key> <value>");
        }
        (None, None) => {
            println!("Config location: {}", config.config_dir.display());
            println!("Model: {}", config.model);
            match ProviderId::from_str(&config.provider) {
                Ok(provider) => println!("Provider: {provider} ({})", provider.auth_mode_label()),
                Err(_) => println!("Provider: {} (custom)", config.provider),
            }
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
                ProviderId::ClaudeCode => {
                    for model in rho_engine::provider::discovery::claude_preset_models() {
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

async fn handle_plugin(action: Option<PluginCommands>, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    match action.unwrap_or(PluginCommands::List) {
        PluginCommands::List => {
            super::plugin::listing::handle_list(config)?;
        }
        PluginCommands::Remove { name, keep_binary } => {
            super::plugin::remove::handle_remove(config, &name, keep_binary).await?;
        }
        PluginCommands::Inspect { capability } => {
            super::plugin::listing::handle_inspect(config, capability.as_deref());
        }
        PluginCommands::Install { target, force } => {
            super::plugin::install::handle_install(config, &target, force).await?;
        }
        PluginCommands::Update { target } => match target.as_deref() {
            None | Some("all") => {
                super::plugin::update::handle_update_all(config).await?;
            }
            Some(plugin) => {
                super::plugin::update::handle_update_plugin(config, plugin).await?;
            }
        },
    }
    Ok(())
}
