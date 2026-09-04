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
use terminal::prompt_password;

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

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let (target, method) = match provider {
        Some(name) => (resolve_provider_name(Some(name), &config.provider), None),
        None => {
            let method = prompt_select_auth_method()?;
            let target = match method {
                AuthMethod::OAuth => prompt_select_oauth_provider()?,
                AuthMethod::ApiKey => prompt_select_api_key_provider(config)?,
            };
            (target, Some(method))
        }
    };

    if target == "local" {
        println!("Local models run offline and do not require credentials.");
        return Ok(());
    }

    if let Ok(id) = ProviderId::from_str(&target) {
        match method {
            Some(AuthMethod::OAuth) => {
                return perform_oauth_and_save(id, config, auth_store).await;
            }
            Some(AuthMethod::ApiKey) => {}
            None => match id {
                ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Antigravity => {
                    return perform_oauth_and_save(id, config, auth_store).await;
                }
                ProviderId::OpenRouter => {
                    let chosen = prompt_auth_method("OpenRouter")?;
                    if chosen == AuthMethod::OAuth {
                        return perform_oauth_and_save(id, config, auth_store).await;
                    }
                }
                _ => {}
            },
        }
    }

    let key = prompt_password(&format!("Enter API key for {target}:"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(&target, key)?;
    crate::repl::interactive::spawn_background_model_refresh(config, auth_store);
    println!("Stored API key for {target}");
    Ok(())
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
            #[cfg(feature = "ui")]
            {
                inquire::Select::new("Select provider credentials to remove:", configured)
                    .prompt()
                    .map_err(|_| AppError::Cancelled("Logout cancelled".to_string()))?
            }
            #[cfg(not(feature = "ui"))]
            {
                configured.first().cloned().unwrap_or_default()
            }
        }
    };

    auth_store.remove_key(&target)?;
    println!("Removed stored credentials for {target}");
    Ok(())
}
