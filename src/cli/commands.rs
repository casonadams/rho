//! CLI subcommand execution (config, models, mcp, login, logout).

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::Commands;

async fn handle_update_command(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    super::self_update::handle_self_update(config).await?;
    Ok(())
}

async fn handle_basic_commands(
    cmd: &Commands,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<bool, Box<dyn std::error::Error>> {
    match cmd {
        Commands::Login { provider, key_stdin } => {
            super::auth::login_provider(provider.as_deref(), *key_stdin, config, auth_store).await?;
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
            handle_models(config, auth_store).await?;
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
    if handle_basic_commands(&cmd, config, auth_store).await? {
        return Ok(());
    }
    match cmd {
        Commands::Mcp { action } => super::mcp::handle_mcp(action, config).await?,
        Commands::Update => handle_update_command(config).await?,
        Commands::Stats { last } => super::stats::handle_stats(last, &config.sessions_dir)?,
        Commands::Worktree { action } => super::worktree::handle_worktree(action, config).await?,
        _ => {}
    }
    Ok(())
}

pub(crate) fn format_config_summary(config: &Config) -> Vec<String> {
    let provider_label = match config.route().connection.provider_id() {
        Some(provider) => format!("Provider: {} ({})", config.provider, provider.auth_mode_label()),
        None => format!("Provider: {} (custom)", config.provider),
    };
    vec![
        format!("Config location: {}", config.config_dir.display()),
        format!("Model: {}", config.model),
        provider_label,
        format!("Max turns: {}", config.max_turns),
        format!("Context window messages: {}", config.context_window_messages),
        format!("Compaction max bytes: {}", config.compaction_max_bytes),
    ]
}

fn print_config_summary(config: &Config) {
    for line in format_config_summary(config) {
        println!("{line}");
    }
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
        (None, None) => print_config_summary(config),
        _ => return Err("Usage: rho config <key> <value>".into()),
    }
    Ok(())
}

pub(crate) fn format_model_entries(
    models: &[rho_engine::provider::DiscoveredModel],
    fallback_model: &str,
) -> Vec<String> {
    if models.is_empty() {
        vec![format!("  - {fallback_model}")]
    } else {
        models
            .iter()
            .map(|m| {
                if m.description.is_empty() {
                    format!("  - {}", m.id)
                } else {
                    format!("  - {} ({})", m.id, m.description)
                }
            })
            .collect()
    }
}

async fn handle_models(config: &Config, auth_store: &AuthStore) -> rho_harness_core::error::Result<()> {
    use rho_engine::provider::discovery::{discover_models, sort_models_newest_first};
    let models = discover_models(&config.provider, config, auth_store.clone()).await?;
    println!("Models for {}:", config.provider);
    for line in format_model_entries(&sort_models_newest_first(models), &config.model) {
        println!("{line}");
    }
    Ok(())
}
