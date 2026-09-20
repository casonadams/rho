//! Interactive CLI login, logout, and terminal OAuth callback handlers.

mod callbacks;
mod provider;
mod terminal;

#[cfg(test)]
mod tests;

pub use callbacks::TerminalOAuthCallbacks;
pub use provider::{
    AuthMethod, prompt_auth_method, prompt_select_api_key_provider, prompt_select_auth_method,
    prompt_select_oauth_provider, resolve_provider_name,
};
pub use terminal::{prompt_password, read_key_from_reader, read_key_from_stdin};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use rho_engine::auth::perform_oauth_login;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

fn provider_login_name(id: ProviderId) -> &'static str {
    match id {
        ProviderId::ChatGpt => "ChatGPT",
        ProviderId::Copilot => "GitHub Copilot",
        ProviderId::Antigravity => "Google Antigravity",
        ProviderId::ClaudeCode => "Claude (Subscription)",
        ProviderId::OpenRouter => "OpenRouter",
        _ => id.as_str(),
    }
}

async fn perform_oauth_and_save(id: ProviderId, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let callbacks = TerminalOAuthCallbacks;
    let cred = perform_oauth_login(id, &callbacks).await?;
    auth_store.set_credential(id.as_str(), cred)?;
    crate::repl::interactive::spawn_background_model_refresh(config, auth_store);
    println!(
        "Logged in to {}. Credentials saved to {}",
        provider_login_name(id),
        config.auth_file.display()
    );
    Ok(())
}

fn resolve_login_target(provider: Option<&str>, config: &Config) -> Result<(String, Option<AuthMethod>)> {
    match provider {
        Some(name) => Ok((resolve_provider_name(Some(name), &config.provider), None)),
        None => {
            let method = prompt_select_auth_method()?;
            let target = match method {
                AuthMethod::OAuth => prompt_select_oauth_provider()?,
                AuthMethod::ApiKey => prompt_select_api_key_provider(config)?,
            };
            Ok((target, Some(method)))
        }
    }
}

async fn try_default_oauth_login(id: ProviderId, config: &Config, auth_store: &mut AuthStore) -> Result<bool> {
    match id {
        ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Antigravity | ProviderId::ClaudeCode => {
            perform_oauth_and_save(id, config, auth_store).await?;
            Ok(true)
        }
        ProviderId::OpenRouter => {
            if prompt_auth_method("OpenRouter")? == AuthMethod::OAuth {
                perform_oauth_and_save(id, config, auth_store).await?;
                return Ok(true);
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

async fn try_oauth_login(
    id: ProviderId,
    method: Option<AuthMethod>,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<bool> {
    match method {
        Some(AuthMethod::OAuth) => {
            perform_oauth_and_save(id, config, auth_store).await?;
            Ok(true)
        }
        Some(AuthMethod::ApiKey) => Ok(false),
        None => try_default_oauth_login(id, config, auth_store).await,
    }
}

fn login_api_key(target: &str, key_stdin: bool, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    use std::io::IsTerminal;
    let key = if key_stdin || !std::io::stdin().is_terminal() {
        read_key_from_stdin()?
    } else {
        prompt_password(&format!("Enter API key for {target}:"))?
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(target, key)?;
    crate::repl::interactive::spawn_background_model_refresh(config, auth_store);
    println!("Stored API key for {target}");
    Ok(())
}

pub async fn login_provider(
    provider: Option<&str>,
    key_stdin: bool,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<()> {
    if key_stdin && provider.is_none() {
        return Err(AppError::Auth(
            "Provider name required when using --key-stdin (e.g. `rho login gemini --key-stdin`)".to_string(),
        ));
    }

    let (target, method) = resolve_login_target(provider, config)?;
    if target == "local" {
        println!("Local models run offline and do not require credentials.");
        return Ok(());
    }
    if !key_stdin
        && let Ok(id) = ProviderId::from_str(&target)
        && try_oauth_login(id, method, config, auth_store).await?
    {
        return Ok(());
    }
    login_api_key(&target, key_stdin, config, auth_store)
}

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let target = match provider {
        Some(name) => resolve_provider_name(Some(name), &config.provider),
        None => {
            let configured = auth_store.list_configured_providers();
            if configured.is_empty() {
                println!("No stored credentials to remove.");
                return Ok(());
            }
            println!("\nSelect provider credentials to remove:");
            for (i, p) in configured.iter().enumerate() {
                println!("  {}. {p}", i + 1);
            }
            use std::io::Write;
            print!("Enter choice [1-{}]: ", configured.len());
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| AppError::Other(e.into()))?;
            let idx = input
                .trim()
                .parse::<usize>()
                .map_err(|_| AppError::Cancelled("Logout cancelled".to_string()))?;
            if idx >= 1 && idx <= configured.len() {
                configured[idx - 1].clone()
            } else {
                return Err(AppError::Cancelled("Logout cancelled".to_string()));
            }
        }
    };

    auth_store.remove_key(&target)?;
    println!("Removed stored credentials for {target}");
    Ok(())
}
