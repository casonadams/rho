use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use rho_core::provider::ProviderId;
use std::str::FromStr;

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let name = resolve_provider_name(provider, &config.provider);
    let key = prompt_password(&format!("Enter API key for {name}:"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(&name, key)?;
    println!("Stored API key for {name}");
    Ok(())
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

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let name = resolve_provider_name(provider, &config.provider);
    auth_store.remove_key(&name)?;
    println!("Removed API key for {name}");
    Ok(())
}

/// Built-in provider names canonicalize (aliases collapse to their enum arm);
/// any other name is kept so custom config providers can store keys.
fn resolve_provider_name(requested: Option<&str>, configured: &str) -> String {
    let requested = requested.unwrap_or(configured).trim().to_ascii_lowercase();
    ProviderId::from_str(&requested)
        .map(|id| id.as_str().to_string())
        .unwrap_or(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_names_canonicalize_and_custom_names_are_kept() {
        assert_eq!(resolve_provider_name(Some("Google"), "anthropic"), "gemini");
        assert_eq!(
            resolve_provider_name(Some("google-antigravity"), "anthropic"),
            "antigravity"
        );
        assert_eq!(resolve_provider_name(None, "GROQ"), "groq");
        assert_eq!(resolve_provider_name(Some("acme"), "anthropic"), "acme");
        assert_eq!(resolve_provider_name(Some("Acme Cloud"), "anthropic"), "acme cloud");
    }
}
