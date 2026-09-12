//! CLI subcommand execution (config, models, mcp, login, logout).

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::Commands;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

async fn handle_update_command(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    super::self_update::handle_self_update(config).await?;
    Ok(())
}

async fn handle_basic_commands(
    cmd: &Commands,
    (config, auth_store): (&Config, &mut AuthStore),
) -> Result<bool, Box<dyn std::error::Error>> {
    match cmd {
        Commands::Login { provider } => {
            super::auth::login_provider(provider.as_deref(), config, auth_store).await?;
            Ok(true)
        }
        Commands::Logout { provider } => {
            super::auth::logout_provider(provider.as_deref(), config, auth_store)?;
            Ok(true)
        }
        Commands::Config { key, value } => {
            handle_config(key.clone(), value.clone(), config).await?;
            Ok(true)
        }
        Commands::Models => {
            handle_models(config);
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub async fn handle_command(
    cmd: Commands,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    if handle_basic_commands(&cmd, (config, auth_store)).await? {
        return Ok(());
    }
    match cmd {
        Commands::Mcp { action } => super::mcp::handle_mcp(action, config, auth_store).await?,
        Commands::Update => handle_update_command(config).await?,
        _ => {}
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

fn print_provider_models(provider: ProviderId, config_model: &str) {
    match provider {
        ProviderId::Anthropic => {
            println!("  - claude-3-7-sonnet-20250219\n  - claude-3-5-sonnet-20241022\n  - claude-3-5-haiku-20241022");
        }
        ProviderId::OpenAi => println!("  - gpt-6-astra\n  - gpt-4o\n  - gpt-4o-mini\n  - o1\n  - o3-mini"),
        ProviderId::Gemini => println!("  - gemini-2.0-flash\n  - gemini-1.5-pro\n  - gemini-1.5-flash"),
        ProviderId::ChatGpt => {
            for model in rho_engine::provider::discovery::chatgpt_codex_models() {
                println!("  - {} ({})", model.id, model.description);
            }
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
        ProviderId::DeepSeek => println!("  - deepseek-chat\n  - deepseek-reasoner"),
        _ => println!("  - {config_model}"),
    }
}

fn handle_models(config: &Config) {
    match ProviderId::from_str(&config.provider) {
        Ok(provider) => {
            println!("Models for {provider}:");
            print_provider_models(provider, &config.model);
        }
        Err(_) => {
            println!("Models for {} (custom):\n  - {}", config.provider, config.model);
        }
    }
}
