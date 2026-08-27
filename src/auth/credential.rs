//! Credential data shape and persistence layer.
//!
//! Owns the [`Credential`] enum (the redacted-on-debug credential record) and the
//! [`AuthStore`] that maps provider ids to credentials on disk with private file
//! permissions. The on-disk helpers ([`set_private_file_permissions`],
//! [`non_empty`]) are file-local because nothing outside this module needs them.

use crate::engine::provider::{CredentialStrategy, ProviderId};
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Credential {
    #[serde(rename = "api_key")]
    ApiKey { key: String },
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
        endpoint: Option<String>,
    },
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { .. } => f.write_str("Credential::ApiKey([REDACTED])"),
            Self::OAuth { .. } => f.write_str("Credential::OAuth([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    pub credentials: HashMap<String, Credential>,
    #[serde(skip)]
    path: PathBuf,
}

impl AuthStore {
    pub fn load(path: &Path) -> Result<Self> {
        let mut store = if path.exists() {
            let data = std::fs::read_to_string(path)
                .map_err(|error| AppError::Auth(format!("Failed to read {}: {error}", path.display())))?;
            serde_json::from_str::<Self>(&data)
                .map_err(|_| AppError::Auth(format!("Credential store {} is malformed", path.display())))?
        } else {
            Self::default()
        };
        store.path = path.to_path_buf();
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)
            .map_err(|error| AppError::Auth(format!("Failed to serialize auth store: {error}")))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        set_private_file_permissions(&self.path)?;
        Ok(())
    }

    pub fn set_api_key(&mut self, provider: &str, key: String) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        if provider.credential_strategy() != CredentialStrategy::ApiKey {
            return Err(AppError::Auth(format!(
                "{provider} does not accept API keys in the rho credential store"
            )));
        }
        self.credentials
            .insert(provider.as_str().to_string(), Credential::ApiKey { key });
        self.save()
    }

    pub fn remove_provider_entry(&mut self, provider: &str) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        self.credentials.remove(provider.as_str());
        self.save()
    }

    pub fn get_key(&self, provider: &str) -> Result<Option<String>> {
        self.get_key_with(provider, |name| std::env::var(name).ok())
    }

    pub(crate) fn secret_values(&self) -> Vec<String> {
        self.credentials
            .values()
            .flat_map(|credential| match credential {
                Credential::ApiKey { key } => vec![key.clone()],
                Credential::OAuth {
                    access_token,
                    refresh_token,
                    ..
                } => {
                    let mut values = vec![access_token.clone()];
                    values.extend(refresh_token.clone());
                    values
                }
            })
            .filter(|value| !value.is_empty())
            .collect()
    }

    pub(crate) fn get_key_with<F>(&self, provider: &str, get_env: F) -> Result<Option<String>>
    where
        F: Fn(&str) -> Option<String>,
    {
        let provider = ProviderId::from_str(provider)?;
        if provider.credential_strategy() != CredentialStrategy::ApiKey {
            return Err(AppError::Auth(format!(
                "{provider} does not expose OAuth credentials through the API-key interface"
            )));
        }

        if let Some(value) = provider.api_key_env().and_then(&get_env).and_then(non_empty) {
            return Ok(Some(value));
        }

        let generic_name = format!("{}_API_KEY", provider.as_str().to_ascii_uppercase());
        if let Some(value) = get_env(&generic_name).and_then(non_empty) {
            return Ok(Some(value));
        }

        match self.credentials.get(provider.as_str()) {
            Some(Credential::ApiKey { key }) => Ok(non_empty(key.clone())),
            Some(Credential::OAuth { .. }) => Err(AppError::Auth(format!(
                "Legacy OAuth credential found for {provider}; remove it and use subscription login"
            ))),
            None => Ok(None),
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(unix)]
pub(crate) fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
