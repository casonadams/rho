//! Secure persistent credential store supporting API keys and OAuth tokens.

mod io;

use super::oauth::refresh_oauth_token;
use super::resolver::{resolve_secret_value, resolve_secret_value_async};
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    #[serde(skip)]
    file_path: PathBuf,
    credentials: HashMap<String, StoredCredential>,
}

impl AuthStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();
        let credentials = io::load_credentials(&file_path)?;
        Ok(Self { file_path, credentials })
    }

    pub async fn load_async(path: impl AsRef<Path>) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();
        let credentials = io::load_credentials_async(&file_path).await?;
        Ok(Self { file_path, credentials })
    }

    pub fn get_key_sync(&self, provider: &str) -> Result<Option<String>> {
        match self.credentials.get(provider) {
            Some(StoredCredential::ApiKey { key, .. }) => resolve_secret_value(key).map(Some),
            Some(oauth) => Ok(Some(oauth.raw_secret().to_string())),
            None => resolve_env_key(provider),
        }
    }

    pub async fn get_key(&mut self, provider: &str) -> Result<Option<String>> {
        self.resolve_key(provider, false).await
    }

    async fn resolve_key(&mut self, provider: &str, force_refresh: bool) -> Result<Option<String>> {
        let Some(cred) = self.credentials.get(provider) else {
            return resolve_env_key(provider);
        };
        match cred {
            StoredCredential::ApiKey { key, .. } => resolve_secret_value_async(key).await.map(Some),
            StoredCredential::OAuth { .. } if force_refresh || cred.is_expired(60) => {
                self.refresh_stored(provider).await.map(Some)
            }
            StoredCredential::OAuth { .. } => Ok(Some(cred.raw_secret().to_string())),
        }
    }

    async fn refresh_stored(&mut self, provider: &str) -> Result<String> {
        let provider_id = ProviderId::from_str(provider)
            .map_err(|e| AppError::Auth(format!("Unknown provider '{provider}': {e}")))?;
        let cred = self
            .credentials
            .get(provider)
            .ok_or_else(|| AppError::Auth(format!("No credential found for provider '{provider}'")))?;
        let refreshed = refresh_oauth_token(provider_id, cred).await?;
        let access = refreshed.raw_secret().to_string();
        self.credentials.insert(provider.to_string(), refreshed);
        self.save_async().await?;
        Ok(access)
    }

    pub async fn force_refresh(&mut self, provider: &str) -> Result<Option<String>> {
        self.resolve_key(provider, true).await
    }

    pub fn get_credential(&self, provider: &str) -> Option<&StoredCredential> {
        self.credentials.get(provider)
    }

    pub fn set_credential(&mut self, provider: &str, cred: StoredCredential) -> Result<()> {
        self.credentials.insert(provider.to_string(), cred);
        self.save()
    }

    pub async fn set_credential_async(&mut self, provider: &str, cred: StoredCredential) -> Result<()> {
        self.credentials.insert(provider.to_string(), cred);
        self.save_async().await
    }

    pub fn set_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_credential(provider, StoredCredential::api_key(key.into()))
    }

    pub async fn set_key_async(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_credential_async(provider, StoredCredential::api_key(key.into()))
            .await
    }

    pub fn set_api_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_key(provider, key)
    }

    pub async fn set_api_key_async(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_key_async(provider, key).await
    }

    pub fn remove_key(&mut self, provider: &str) -> Result<()> {
        self.credentials.remove(provider);
        self.save()
    }

    pub async fn remove_key_async(&mut self, provider: &str) -> Result<()> {
        self.credentials.remove(provider);
        self.save_async().await
    }

    pub async fn save_async(&self) -> Result<()> {
        io::save_credentials_async(&self.file_path, &self.credentials).await
    }

    pub fn list_stored_providers(&self) -> Vec<String> {
        let mut list: Vec<String> = self.credentials.keys().cloned().collect();
        list.sort();
        list
    }

    pub fn list_configured_providers(&self) -> Vec<String> {
        let mut list: Vec<String> = self.credentials.keys().cloned().collect();
        for id in ProviderId::API_KEY_PROVIDERS {
            if let Some(env_name) = id.api_key_env()
                && std::env::var(env_name).is_ok_and(|v| !v.trim().is_empty())
                && !list.contains(&id.as_str().to_string())
            {
                list.push(id.as_str().to_string());
            }
        }
        list.sort();
        list
    }

    pub async fn resolved_secret_values(&self) -> Vec<String> {
        let mut values = Vec::new();
        for cred in self.credentials.values() {
            match cred {
                StoredCredential::ApiKey { key, .. } => values.extend(resolve_secret_value_async(key).await.ok()),
                StoredCredential::OAuth { .. } => values.push(cred.raw_secret().to_string()),
            }
        }
        values
    }

    pub fn oauth_secret_values(&self) -> Vec<String> {
        self.credentials
            .values()
            .filter_map(|cred| match cred {
                StoredCredential::OAuth {
                    access_token,
                    refresh_token,
                    ..
                } => Some(std::iter::once(access_token.clone()).chain(refresh_token.clone())),
                StoredCredential::ApiKey { .. } => None,
            })
            .flatten()
            .collect()
    }

    fn save(&self) -> Result<()> {
        io::save_credentials(&self.file_path, &self.credentials)
    }
}

fn resolve_env_key(provider: &str) -> Result<Option<String>> {
    if let Ok(id) = ProviderId::from_str(provider)
        && let Some(env_name) = id.api_key_env()
        && let Ok(val) = std::env::var(env_name)
        && !val.trim().is_empty()
    {
        return resolve_secret_value(&val).map(Some);
    }
    let generic_env = format!("{}_API_KEY", provider.to_ascii_uppercase().replace('-', "_"));
    if let Ok(val) = std::env::var(&generic_env)
        && !val.trim().is_empty()
    {
        return resolve_secret_value(&val).map(Some);
    }

    Ok(None)
}
