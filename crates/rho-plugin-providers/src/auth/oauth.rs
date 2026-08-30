//! OAuth subscription flow.
//!
//! Owns the [`OAuthManager`] (token directory layout, login/reload/logout for
//! the two subscription providers — ChatGPT and Copilot), the rig OAuth client
//! builders ([`chatgpt_client`], [`copilot_client`]), and the upstream-error
//! normaliser [`map_oauth_error`] that converts raw rig messages into safe,
//! redacted user-facing errors. The private [`token_files`] and
//! [`secure_token_files`] helpers are file-local because nothing else enumerates
//! token files per provider.

use crate::auth::credential::set_private_file_permissions;
use rho_core::error::{AppError, Result};
use rho_core::provider::{CredentialStrategy, ProviderId};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct OAuthManager {
    token_root: PathBuf,
}

impl OAuthManager {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            token_root: config_dir.join("tokens"),
        }
    }

    pub fn token_dir(&self, provider: ProviderId) -> Result<PathBuf> {
        if provider.credential_strategy() != CredentialStrategy::SubscriptionOAuth {
            return Err(AppError::Auth(format!("{provider} is not a subscription provider")));
        }
        Ok(self.token_root.join(provider.as_str()))
    }

    pub async fn login(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.prepare_token_dir(provider)?;
        let result = match provider {
            ProviderId::ChatGpt => chatgpt_client(&token_dir, true)?.authorize().await,
            ProviderId::Copilot => copilot_client(&token_dir, true)?.authorize().await,
            _ => {
                return Err(AppError::Auth(format!(
                    "{provider} does not support subscription login"
                )));
            }
        };
        result.map_err(|error| map_oauth_error(provider, &error.to_string()))?;
        secure_token_files(provider, &token_dir)?;
        Ok(())
    }

    pub async fn reload(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.prepare_token_dir(provider)?;
        let result = match provider {
            ProviderId::ChatGpt => chatgpt_client(&token_dir, false)?.authorize().await,
            ProviderId::Copilot => copilot_client(&token_dir, false)?.authorize().await,
            _ => {
                return Err(AppError::Auth(format!(
                    "{provider} does not support subscription login"
                )));
            }
        };
        result.map_err(|error| map_oauth_error(provider, &error.to_string()))?;
        secure_token_files(provider, &token_dir)?;
        Ok(())
    }

    pub async fn refresh_if_needed(&self, provider: ProviderId) -> Result<()> {
        self.reload(provider).await
    }

    pub fn logout(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.token_dir(provider)?;
        for file in token_files(provider, &token_dir) {
            match std::fs::remove_file(file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match std::fs::remove_dir(&token_dir) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    pub(crate) fn prepare_token_dir(&self, provider: ProviderId) -> Result<PathBuf> {
        let token_dir = self.token_dir(provider)?;
        std::fs::create_dir_all(&token_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(token_dir)
    }
}

pub fn chatgpt_client(token_dir: &Path, interactive: bool) -> Result<rig::providers::chatgpt::Client> {
    rig::providers::chatgpt::Client::builder()
        .oauth()
        .token_dir(token_dir)
        .allow_device_flow(interactive)
        .on_device_code(|prompt| {
            println!("Open {} and enter code {}", prompt.verification_uri, prompt.user_code);
        })
        .build()
        .map_err(|_| AppError::Auth("Failed to initialize ChatGPT OAuth".to_string()))
}

pub fn copilot_client(token_dir: &Path, interactive: bool) -> Result<rig::providers::copilot::Client> {
    rig::providers::copilot::Client::builder()
        .oauth()
        .token_dir(token_dir)
        .allow_device_flow(interactive)
        .on_device_code(|prompt| {
            println!("Open {} and enter code {}", prompt.verification_uri, prompt.user_code);
        })
        .build()
        .map_err(|_| AppError::Auth("Failed to initialize Copilot OAuth".to_string()))
}

fn token_files(provider: ProviderId, token_dir: &Path) -> Vec<PathBuf> {
    match provider {
        ProviderId::ChatGpt => vec![token_dir.join("auth.json")],
        ProviderId::Copilot => vec![token_dir.join("access-token"), token_dir.join("api-key.json")],
        _ => Vec::new(),
    }
}

fn secure_token_files(provider: ProviderId, token_dir: &Path) -> Result<()> {
    for file in token_files(provider, token_dir) {
        if file.exists() {
            set_private_file_permissions(&file)?;
        }
    }
    Ok(())
}

pub(crate) fn map_oauth_error(provider: ProviderId, message: &str) -> AppError {
    let normalized = message.to_ascii_lowercase();
    let detail = if normalized.contains("denied") || normalized.contains("cancel") {
        "device authorization was cancelled or denied"
    } else if normalized.contains("did not include a token") || normalized.contains("entitlement") {
        "the account has no usable subscription entitlement"
    } else if normalized.contains("sign-in required")
        || normalized.contains("invalid_grant")
        || normalized.contains("401")
    {
        "stored credentials are missing, stale, or revoked; log in again"
    } else if normalized.contains("timed out") || normalized.contains("expired") {
        "device authorization expired or timed out"
    } else if normalized.contains("api key") || normalized.contains("token exchange") {
        "subscription token exchange failed"
    } else {
        "authentication failed"
    };
    AppError::Auth(format!("{provider} {detail}"))
}
