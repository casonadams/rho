use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use rho_core::provider::ProviderId;
use std::str::FromStr;

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    login_api_key(provider, auth_store).await
}

#[cfg(feature = "ui")]
fn prompt_password(prompt: &str) -> Result<String> {
    inquire::Password::new(prompt)
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

#[cfg(not(feature = "ui"))]
fn prompt_password(prompt: &str) -> Result<String> {
    use std::io::BufRead;
    println!("{prompt}");
    let mut buffer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut buffer)
        .map_err(|e| AppError::Other(e.into()))?;
    Ok(buffer.trim_end_matches(&['\r', '\n'][..]).to_string())
}

pub async fn login_api_key(provider: ProviderId, auth_store: &mut AuthStore) -> Result<()> {
    let key = prompt_password(&format!("Enter API key for {provider}:"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(provider.as_str(), key)?;
    println!("Stored API key for {provider}");
    Ok(())
}

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    auth_store.remove_key(provider.as_str())?;
    println!("Removed API key for {provider}");
    Ok(())
}

pub fn select_provider(requested: Option<&str>, configured: &str) -> Result<ProviderId> {
    ProviderId::from_str(requested.unwrap_or(configured))
}
