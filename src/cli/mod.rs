pub mod auth;

#[cfg(test)]
mod tests;

pub use auth::{login_provider, logout_provider};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::{Cli, Commands};
use crate::engine::provider::ProviderId;
use crate::repl::ReplSession;
use crate::ui::TerminalRenderer;
use rho_core::session::SessionManager;
use std::io::Read;
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
                auth::handle_auth_action(action.unwrap_or(crate::config::cli::AuthCommands::Set), &config)?;
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

    let resume_target = if cli.r#continue {
        let cwd = std::env::current_dir()?;
        SessionManager::last_session_for_cwd(&config.sessions_dir, &cwd)?
    } else {
        cli.resume
    };

    if let Some(prompt) = prompt_text {
        let engine = crate::platform::agent_engine(config, auth_store, resume_target.as_deref()).await?;
        #[cfg(feature = "ui")]
        let presenter: std::sync::Arc<dyn rho_core::presentation::Presenter> =
            std::sync::Arc::new(TerminalRenderer::default());
        #[cfg(not(feature = "ui"))]
        let presenter: std::sync::Arc<dyn rho_core::presentation::Presenter> =
            std::sync::Arc::new(rho_core::presentation::StructuredPresenter::stdout());

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
            let mut session = ReplSession::new(config, auth_store, resume_target);
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

pub(crate) fn parse_replacements(
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
