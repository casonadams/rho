use crate::auth::{
    AuthStore, OAuthManager, PendingApiKey, VerificationStatus, cancellable_oauth, store_api_key_after_verification,
};
use crate::config::Config;
use crate::engine::provider::{CredentialStrategy, ProviderId, RigCredentialVerifier};
use crate::error::{AppError, Result};
use std::future::Future;
use std::path::Path;
use std::str::FromStr;

pub async fn login_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    let strategy = crate::engine::provider::registry::ProviderRegistry::builtins()
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

pub async fn login_subscription<F, Fut, C>(provider: ProviderId, login: F, cancel: C) -> Result<()>
where
    F: FnOnce(ProviderId) -> Fut,
    Fut: Future<Output = Result<()>>,
    C: Future<Output = std::io::Result<()>>,
{
    cancellable_oauth(provider, login(provider), cancel).await?;
    println!("Authenticated {provider} subscription with Rig OAuth");
    Ok(())
}

pub async fn login_api_key(provider: ProviderId, auth_store: &mut AuthStore) -> Result<()> {
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

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let provider = select_provider(provider, &config.provider)?;
    let strategy = crate::engine::provider::registry::ProviderRegistry::builtins()
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

pub fn select_provider(requested: Option<&str>, configured: &str) -> Result<ProviderId> {
    ProviderId::from_str(requested.unwrap_or(configured))
}

pub fn handle_auth_action(action: crate::config::cli::AuthCommands, config: &Config) -> Result<()> {
    match action {
        crate::config::cli::AuthCommands::Set => set_ollama_cloud_key(&config.config_dir),
        crate::config::cli::AuthCommands::Remove => remove_ollama_cloud_key(&config.config_dir),
    }
}

pub fn set_ollama_cloud_key(config_dir: &Path) -> Result<()> {
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

pub fn remove_ollama_cloud_key(config_dir: &Path) -> Result<()> {
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

pub fn write_ollama_cloud_key(config_dir: &Path, key: &str) -> Result<()> {
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
