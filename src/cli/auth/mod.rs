//! Interactive CLI login, logout, and terminal OAuth callback handlers.

mod callbacks;
mod provider;
mod terminal;

#[cfg(test)]
mod tests;

pub use callbacks::TerminalOAuthCallbacks;
pub use provider::{prompt_select_provider, resolve_provider_name};
use terminal::prompt_password;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use rho_engine::auth::perform_oauth_login;
use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let target = match provider {
        Some(name) => resolve_provider_name(Some(name), &config.provider),
        None => prompt_select_provider(config)?,
    };

    if let Ok(id) = ProviderId::from_str(&target) {
        match id {
            ProviderId::ChatGpt => {
                let callbacks = TerminalOAuthCallbacks;
                let cred = perform_oauth_login(id, &callbacks).await?;
                auth_store.set_credential(id.as_str(), cred)?;
                println!(
                    "Logged in to ChatGPT. Credentials saved to {}",
                    config.auth_file.display()
                );
                return Ok(());
            }
            ProviderId::Copilot => {
                let callbacks = TerminalOAuthCallbacks;
                let cred = perform_oauth_login(id, &callbacks).await?;
                auth_store.set_credential(id.as_str(), cred)?;
                println!(
                    "Logged in to GitHub Copilot. Credentials saved to {}",
                    config.auth_file.display()
                );
                return Ok(());
            }
            ProviderId::Antigravity => {
                let callbacks = TerminalOAuthCallbacks;
                let cred = perform_oauth_login(id, &callbacks).await?;
                auth_store.set_credential(id.as_str(), cred)?;
                println!(
                    "Logged in to Google Antigravity. Credentials saved to {}",
                    config.auth_file.display()
                );
                return Ok(());
            }
            ProviderId::Local => {
                println!("Local models run offline and do not require credentials.");
                return Ok(());
            }
            _ => {}
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
