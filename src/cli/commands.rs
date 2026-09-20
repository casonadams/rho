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
            handle_models(config, auth_store).await;
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
        Commands::Index { path, force } => handle_index_command(path, force).await?,
        Commands::Serve { workspace, port, name } => {
            super::serve::handle_serve(workspace, port, name, config, auth_store).await?
        }
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
            println!(
                "Semantic search: {}",
                if config.semantic_search { "enabled" } else { "disabled" }
            );
        }
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

async fn handle_index_command(path: Option<String>, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };
    println!("Indexing workspace: {}", workspace.display());
    let embedder = rho_engine::rag::LocalEmbedder::new();
    let summary = rho_engine::rag::index_workspace(&workspace, force, &embedder)
        .await
        .map_err(|e| format!("Indexing failed: {e}"))?;
    println!(
        "Index complete: {} files scanned, {} total chunks ({} new, {} reused)",
        summary.files_indexed, summary.total_chunks, summary.new_chunks, summary.reused_chunks
    );
    println!(
        "Index saved to: {}",
        rho_engine::rag::CodebaseIndex::index_path(&workspace).display()
    );
    Ok(())
}

async fn handle_models(config: &Config, auth_store: &AuthStore) {
    let mut store = rho_engine::provider::ModelStore::load(config.config_dir.join("models-store.json"));
    let cached = store.get_models(&config.provider).cloned();

    let is_local = config.provider == "local" || config.provider == "ollama";
    let models = if is_local || cached.is_none() {
        if let Ok(provider_id) = ProviderId::from_str(&config.provider)
            && let Ok(discovered) =
                rho_engine::provider::discovery::discover_provider_models(provider_id, auth_store).await
            && !discovered.is_empty()
        {
            let _ = store.set_models_async(&config.provider, discovered.clone()).await;
            discovered
        } else {
            cached.unwrap_or_else(|| rho_engine::provider::discovery::default_presets_for(&config.provider))
        }
    } else {
        cached.unwrap_or_else(|| rho_engine::provider::discovery::default_presets_for(&config.provider))
    };

    println!("Models for {}:", config.provider);
    for line in format_model_entries(&models, &config.model) {
        println!("{line}");
    }
}
