//! Secure persistent credential store supporting API keys and OAuth tokens.

use super::oauth::refresh_oauth_token;
use super::resolver::resolve_secret_value;
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    #[serde(skip)]
    file_path: PathBuf,
    credentials: HashMap<String, StoredCredential>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawStoredEntry {
    Structured(StoredCredential),
    LegacyString(String),
}

impl AuthStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self {
                file_path: path.to_path_buf(),
                credentials: HashMap::new(),
            });
        }
        let content =
            std::fs::read_to_string(path).map_err(|e| AppError::Auth(format!("Failed to read auth file: {e}")))?;

        let raw_map: HashMap<String, RawStoredEntry> = serde_json::from_str(&content).unwrap_or_default();

        let mut credentials = HashMap::new();
        for (k, entry) in raw_map {
            match entry {
                RawStoredEntry::Structured(mut c) => {
                    if let StoredCredential::OAuth {
                        ref access_token,
                        ref mut account_id,
                        ..
                    } = c
                        && account_id.is_none()
                        && k == "chatgpt"
                    {
                        *account_id = super::oauth::extract_chatgpt_account_id(access_token);
                    }
                    credentials.insert(k, c);
                }
                RawStoredEntry::LegacyString(s) => {
                    credentials.insert(k, StoredCredential::api_key(s));
                }
            }
        }

        Ok(Self {
            file_path: path.to_path_buf(),
            credentials,
        })
    }

    pub fn get_key_sync(&self, provider: &str) -> Result<Option<String>> {
        if let Some(cred) = self.credentials.get(provider) {
            return match cred {
                StoredCredential::ApiKey { key, .. } => resolve_secret_value(key).map(Some),
                StoredCredential::OAuth { access_token, .. } => Ok(Some(access_token.clone())),
            };
        }

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
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&self.credentials)
            .map_err(|e| AppError::Auth(format!("Failed to serialize auth store: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.file_path)?;
            file.write_all(json.as_bytes())?;
        }

        #[cfg(not(unix))]
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&self.file_path)?;
            file.write_all(json.as_bytes())?;
        }

        Ok(())
    }
}
