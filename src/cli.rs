use crate::auth::{
    AuthStore, OAuthManager, PendingApiKey, VerificationStatus, cancellable_oauth, store_api_key_after_verification,
};
use crate::config::Config;
use crate::config::cli::{Cli, Commands};
use crate::engine::provider::{CredentialStrategy, ProviderId, RigCredentialVerifier};
use crate::error::{AppError, Result};
use crate::repl::ReplSession;
use crate::ui::TerminalRenderer;
use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

pub async fn run_cli() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = <Cli as clap::Parser>::parse();
    let config = Config::load(Some(&cli))?;
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
            Commands::Auth { action } => {
                handle_auth_action(action.unwrap_or(crate::config::cli::AuthCommands::Set), &config)?;
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
                        let provider = ProviderId::from_str(&config.provider)?;
                        println!("Model: {}", config.model);
                        println!("Provider: {provider} ({})", provider.auth_mode_label());
                        println!("Auto approve: {}", config.auto_approve);
                        println!("Max turns: {}", config.max_turns);
                        println!("Context window messages: {}", config.context_window_messages);
                        println!("Compaction max bytes: {}", config.compaction_max_bytes);
                    }
                }
                return Ok(());
            }
            Commands::Models => {
                let provider = ProviderId::from_str(&config.provider)?;
                let catalog = crate::engine::provider::list_models(provider, &auth_store, &config.config_dir).await?;
                println!("Models for {provider} ({})", catalog.source_label());
                for model in catalog.models() {
                    println!("  - {model}");
                }
                return Ok(());
            }
            Commands::Plugin { action } => {
                let cwd = std::env::current_dir().ok();
                match action.unwrap_or(crate::config::cli::PluginCommands::List) {
                    crate::config::cli::PluginCommands::List => {
                        let inspection = crate::plugin::inspection::inspect(&config, cwd.as_deref()).await?;
                        print!("{}", inspection.render());
                    }
                    crate::config::cli::PluginCommands::Inspect { capability } => {
                        let inspection = crate::plugin::inspection::inspect(&config, cwd.as_deref()).await?;
                        if let Some(capability) = capability {
                            let capability = capability.parse::<crate::plugin::capability::CapabilityId>()?;
                            print!("{}", inspection.render_capability(&capability));
                        } else {
                            print!("{}", inspection.render());
                        }
                    }
                    crate::config::cli::PluginCommands::Install { package, replaces } => {
                        let replacements = parse_replacements(replaces)?;
                        println!(
                            "Configured plugins are trusted executables and are not OS-sandboxed; installing {package}"
                        );
                        let manager = crate::plugin::activation::PluginManager::new(
                            crate::plugin::activation::PluginManagerPaths {
                                config_dir: config.config_dir.clone(),
                                cargo_bin: crate::plugin::activation::default_cargo_bin()?,
                            },
                            crate::plugin::activation::SystemCargo,
                            crate::plugin::activation::ProtocolPluginValidator,
                        );
                        let installed = manager.install(&package, replacements).await?;
                        println!(
                            "Installed and configured {} at {}",
                            installed.name,
                            installed.executable.display()
                        );
                    }
                    crate::config::cli::PluginCommands::Remove { name } => {
                        let cargo_bin = crate::plugin::activation::default_cargo_bin()
                            .unwrap_or_else(|_| config.config_dir.join("cargo-bin"));
                        let manager = crate::plugin::activation::PluginManager::new(
                            crate::plugin::activation::PluginManagerPaths {
                                config_dir: config.config_dir.clone(),
                                cargo_bin,
                            },
                            crate::plugin::activation::SystemCargo,
                            crate::plugin::activation::ProtocolPluginValidator,
                        );
                        let removed = manager.remove(&name)?;
                        println!("Removed plugin declaration for {}", removed.name);
                    }
                }
                return Ok(());
            }
        }
    }

    let prompt_text = if let Some(p) = cli.prompt {
        Some(p)
    } else if !atty_check() {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer).ok();
        let trimmed = buffer.trim().to_string();
        if !trimmed.is_empty() { Some(trimmed) } else { None }
    } else {
        None
    };

    if let Some(prompt) = prompt_text {
        let engine = crate::engine::AgentEngine::new(config, auth_store, cli.resume.as_deref()).await?;
        let renderer = TerminalRenderer::default();

        let res = engine
            .run_turn(crate::engine::runner::TurnRequest { prompt: &prompt }, &renderer)
            .await;
        renderer.flush();

        println!();
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let mut session = ReplSession::new(config, auth_store, cli.resume);
        session.run().await?;
        Ok(())
    }
}

pub(crate) async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    let strategy = crate::plugin::provider::ProviderRegistry::builtins()
        .get(provider.as_str())?
        .credential_strategy();
    match strategy {
        CredentialStrategy::ApiKey => login_api_key(provider, auth_store).await,
        CredentialStrategy::SubscriptionOAuth => {
            let manager = OAuthManager::new(&config.config_dir);
            login_subscription(provider, |selected| manager.login(selected), tokio::signal::ctrl_c()).await
        }
        CredentialStrategy::Local => {
            println!("{provider} is local and does not require credential verification");
            Ok(())
        }
    }
}

async fn login_subscription<F, Fut, C>(provider: ProviderId, login: F, cancel: C) -> Result<()>
where
    F: FnOnce(ProviderId) -> Fut,
    Fut: Future<Output = Result<()>>,
    C: Future<Output = std::io::Result<()>>,
{
    cancellable_oauth(provider, login(provider), cancel).await?;
    println!("Authenticated {provider} subscription with Rig OAuth");
    Ok(())
}

async fn login_api_key(provider: ProviderId, auth_store: &mut AuthStore) -> Result<()> {
    let key = inquire::Password::new(&format!("Enter API key for {provider}:"))
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|_| AppError::Cancelled(format!("{provider} login cancelled")))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    let pending = PendingApiKey {
        provider,
        key: key.to_string(),
    };
    let status = store_api_key_after_verification(auth_store, pending, &RigCredentialVerifier).await?;
    match status {
        VerificationStatus::Verified => println!("Verified and stored API key for {provider}"),
        VerificationStatus::Deferred => {
            println!("Stored API key for {provider}; Rig does not support verification, so validation is deferred")
        }
    }
    Ok(())
}

pub(crate) fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    let strategy = crate::plugin::provider::ProviderRegistry::builtins()
        .get(provider.as_str())?
        .credential_strategy();
    match strategy {
        CredentialStrategy::ApiKey => auth_store.remove_provider_entry(provider.as_str())?,
        CredentialStrategy::SubscriptionOAuth => {
            OAuthManager::new(&config.config_dir).logout(provider)?;
            auth_store.remove_provider_entry(provider.as_str())?;
        }
        CredentialStrategy::Local => {}
    }
    println!("Removed only {provider} credentials");
    Ok(())
}

fn select_provider(requested: Option<&str>, configured: &str) -> Result<ProviderId> {
    ProviderId::from_str(requested.unwrap_or(configured))
}

fn handle_auth_action(action: crate::config::cli::AuthCommands, config: &Config) -> Result<()> {
    match action {
        crate::config::cli::AuthCommands::Set => set_ollama_cloud_key(&config.config_dir),
        crate::config::cli::AuthCommands::Remove => remove_ollama_cloud_key(&config.config_dir),
    }
}

fn set_ollama_cloud_key(config_dir: &Path) -> Result<()> {
    println!(
        "Create a key at https://ollama.com/settings/keys, then paste it below to show usage for ollama :cloud models."
    );
    let key = inquire::Password::new("Enter Ollama Cloud API key:")
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|_| AppError::Cancelled("Ollama Cloud login cancelled".to_string()))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    write_ollama_cloud_key(config_dir, key)
}

fn remove_ollama_cloud_key(config_dir: &Path) -> Result<()> {
    let auth_path = config_dir.join("tokens/ollama-cloud/auth.json");
    match std::fs::remove_file(&auth_path) {
        Ok(()) => println!("Removed Ollama Cloud API key"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("No Ollama Cloud API key stored yet");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn write_ollama_cloud_key(config_dir: &Path, key: &str) -> Result<()> {
    let token_dir = config_dir.join("tokens/ollama-cloud");
    std::fs::create_dir_all(&token_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let auth_path = token_dir.join("auth.json");
    let body = serde_json::json!({ "type": "api_key", "key": key });
    std::fs::write(
        &auth_path,
        serde_json::to_vec_pretty(&body).map_err(|e| AppError::Other(e.into()))?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&auth_path, std::fs::Permissions::from_mode(0o600))?;
    }
    println!("Stored Ollama Cloud API key in {}", auth_path.display());
    Ok(())
}

fn parse_replacements(
    replacements: Vec<String>,
) -> std::result::Result<
    std::collections::BTreeSet<crate::plugin::capability::CapabilityId>,
    crate::plugin::capability::CapabilityValidationError,
> {
    replacements.into_iter().map(|value| value.parse()).collect()
}

fn atty_check() -> bool {
    crossterm::tty::IsTty::is_tty(&std::io::stdin())
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn replacement_flags_are_validated_and_deduplicated() {
        let replacements = parse_replacements(vec!["tool:bash".to_string(), "tool:bash".to_string()]).unwrap();
        assert_eq!(replacements.len(), 1);
        assert!(parse_replacements(vec!["not-a-capability".to_string()]).is_err());
    }
}

#[cfg(test)]
mod provider_cli_tests {
    use super::*;

    #[test]
    fn unknown_login_provider_fails_locally() {
        let error = select_provider(Some("unknown-provider"), "anthropic").unwrap_err();
        assert!(error.to_string().contains("unsupported AI provider"));
    }

    #[test]
    fn provider_help_identities_have_distinct_auth_modes() {
        assert_eq!(
            select_provider(Some("openai"), "anthropic")
                .unwrap()
                .credential_strategy(),
            CredentialStrategy::ApiKey
        );
        assert_eq!(
            select_provider(Some("chatgpt"), "anthropic")
                .unwrap()
                .credential_strategy(),
            CredentialStrategy::SubscriptionOAuth
        );
        assert_eq!(
            select_provider(Some("copilot"), "anthropic")
                .unwrap()
                .credential_strategy(),
            CredentialStrategy::SubscriptionOAuth
        );
    }

    #[tokio::test]
    async fn subscription_login_dispatches_chatgpt_and_copilot_without_credentials() {
        for expected in [ProviderId::ChatGpt, ProviderId::Copilot] {
            let selected = std::sync::Arc::new(std::sync::Mutex::new(None));
            let observed = selected.clone();
            login_subscription(
                expected,
                move |provider| {
                    *observed.lock().unwrap() = Some(provider);
                    std::future::ready(Ok(()))
                },
                std::future::pending(),
            )
            .await
            .unwrap();
            assert_eq!(*selected.lock().unwrap(), Some(expected));
        }
    }
}
