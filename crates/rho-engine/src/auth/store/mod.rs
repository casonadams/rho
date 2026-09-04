//! Secure persistent credential store supporting API keys and OAuth tokens.

mod io;

use super::oauth::refresh_oauth_token;
use super::resolver::resolve_secret_value;
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

    pub fn get_key_sync(&self, provider: &str) -> Result<Option<String>> {
        if let Some(cred) = self.credentials.get(provider) {
            return match cred {
                StoredCredential::ApiKey { key, .. } => resolve_secret_value(key).map(Some),
                StoredCredential::OAuth { access_token, .. } => Ok(Some(access_token.clone())),
            };
        }

        resolve_env_key(provider)
    }

    pub async fn get_key(&mut self, provider: &str) -> Result<Option<String>> {
        if let Some(cred) = self.credentials.get(provider) {
            return match cred {
                StoredCredential::ApiKey { key, .. } => resolve_secret_value(key).map(Some),
                StoredCredential::OAuth { .. } => {
                    // Check if token needs refresh (within 60 seconds of expiring)
                    if cred.is_expired(60)
                        && let Ok(provider_id) = ProviderId::from_str(provider)
                        && let Ok(refreshed) = refresh_oauth_token(provider_id, cred).await
                    {
                        let access = refreshed.raw_secret().to_string();
                        self.credentials.insert(provider.to_string(), refreshed);
                        let _ = self.save();
                        return Ok(Some(access));
                    }
                    Ok(Some(cred.raw_secret().to_string()))
                }
            };
        }

        resolve_env_key(provider)
    }

    pub async fn force_refresh(&mut self, provider: &str) -> Result<Option<String>> {
        if let Some(cred) = self.credentials.get(provider) {
            match cred {
                StoredCredential::OAuth { .. } => {
                    let provider_id = ProviderId::from_str(provider)
                        .map_err(|e| AppError::Auth(format!("Unknown provider '{provider}': {e}")))?;
                    let refreshed = refresh_oauth_token(provider_id, cred).await?;
                    let access = refreshed.raw_secret().to_string();
                    self.credentials.insert(provider.to_string(), refreshed);
                    self.save()?;
                    return Ok(Some(access));
                }
                StoredCredential::ApiKey { key, .. } => return resolve_secret_value(key).map(Some),
            }
        }
        self.get_key(provider).await
    }

    pub fn get_credential(&self, provider: &str) -> Option<&StoredCredential> {
        self.credentials.get(provider)
    }

    pub fn set_credential(&mut self, provider: &str, cred: StoredCredential) -> Result<()> {
        self.credentials.insert(provider.to_string(), cred);
        self.save()
    }

    pub fn set_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_credential(provider, StoredCredential::api_key(key.into()))
    }

    pub fn set_api_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_key(provider, key)
    }

    pub fn remove_key(&mut self, provider: &str) -> Result<()> {
        self.credentials.remove(provider);
        self.save()
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

    pub fn secret_values(&self) -> Vec<String> {
        self.credentials.values().map(|c| c.raw_secret().to_string()).collect()
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
