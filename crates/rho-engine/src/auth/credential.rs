//! Credential data shape and persistence layer.
//!
//! Owns the [`Credential`] enum (the redacted-on-debug credential record) and the
//! [`AuthStore`] that maps provider ids to credentials on disk with private file
//! permissions. The on-disk helpers ([`set_private_file_permissions`],
//! [`non_empty`]) are file-local because nothing outside this module needs them.

use crate::engine::provider::{CredentialStrategy, ProviderId};
use rho_core::error::{AppError, Result};
use rho_sdk::capability::{CapabilityId, PluginId};
use rho_sdk::contract::ScopedCredential;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CredentialScope {
    pub plugin_id: PluginId,
    pub capability_id: CapabilityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CredentialUpdate {
    pub scope: CredentialScope,
    pub expected_generation: Option<u64>,
    pub credential: Credential,
}

impl CredentialScope {
    pub fn builtin_provider(provider: ProviderId) -> Self {
        Self {
            plugin_id: "rho.builtin".parse().unwrap(),
            capability_id: format!("provider:{}", provider.as_str()).parse().unwrap(),
            account_id: None,
        }
    }

    fn storage_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.plugin_id,
            self.capability_id,
            self.account_id.as_deref().unwrap_or("")
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VersionedCredential {
    pub generation: u64,
    pub credential: Credential,
}

impl fmt::Debug for VersionedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VersionedCredential")
            .field("generation", &self.generation)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    pub credentials: HashMap<String, Credential>,
    #[serde(default)]
    scoped_credentials: HashMap<String, VersionedCredential>,
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
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let data = serde_json::to_vec_pretty(self)
            .map_err(|error| AppError::Auth(format!("Failed to serialize auth store: {error}")))?;
        let temporary = parent.join(format!(".rho-auth-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&data)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            set_private_file_permissions(&self.path)?;
            if let Ok(directory) = std::fs::File::open(parent) {
                directory.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }

    pub fn set_api_key(&mut self, provider: &str, key: String) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        if provider.credential_strategy() != CredentialStrategy::ApiKey {
            return Err(AppError::Auth(format!(
                "{provider} does not accept API keys in the rho credential store"
            )));
        }
        let credential = Credential::ApiKey { key };
        self.credentials
            .insert(provider.as_str().to_string(), credential.clone());
        self.replace_scoped(CredentialScope::builtin_provider(provider), credential);
        self.save()
    }

    pub fn remove_provider_entry(&mut self, provider: &str) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        self.credentials.remove(provider.as_str());
        self.scoped_credentials
            .remove(&CredentialScope::builtin_provider(provider).storage_key());
        self.save()
    }

    pub fn get_key(&self, provider: &str) -> Result<Option<String>> {
        self.get_key_with(provider, |name| std::env::var(name).ok())
    }

    pub fn scoped_credential(&self, scope: &CredentialScope) -> Option<(u64, ScopedCredential)> {
        let record = self.scoped_credentials.get(&scope.storage_key())?;
        Some((record.generation, credential_envelope(&record.credential)))
    }

    pub fn compare_and_swap(&mut self, update: CredentialUpdate) -> Result<u64> {
        let CredentialUpdate {
            scope,
            expected_generation,
            credential,
        } = update;
        let key = scope.storage_key();
        let current = self.scoped_credentials.get(&key).map(|record| record.generation);
        if current != expected_generation {
            return Err(AppError::Auth(
                "Credential refresh was stale and was not persisted".to_string(),
            ));
        }
        let generation = current.unwrap_or(0).saturating_add(1);
        self.scoped_credentials
            .insert(key, VersionedCredential { generation, credential });
        self.save()?;
        Ok(generation)
    }

    pub fn remove_scope(&mut self, scope: &CredentialScope) -> Result<()> {
        self.scoped_credentials.remove(&scope.storage_key());
        self.save()
    }

    fn replace_scoped(&mut self, scope: CredentialScope, credential: Credential) {
        let key = scope.storage_key();
        let generation = self
            .scoped_credentials
            .get(&key)
            .map_or(1, |record| record.generation.saturating_add(1));
        self.scoped_credentials
            .insert(key, VersionedCredential { generation, credential });
    }

    pub(crate) fn secret_values(&self) -> Vec<String> {
        self.credentials
            .values()
            .chain(self.scoped_credentials.values().map(|record| &record.credential))
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

fn credential_envelope(credential: &Credential) -> ScopedCredential {
    match credential {
        Credential::ApiKey { key } => ScopedCredential {
            kind: "api_key.v1".to_string(),
            value: serde_json::json!({"key": key}),
        },
        Credential::OAuth {
            access_token,
            refresh_token,
            expires_at,
            endpoint,
        } => ScopedCredential {
            kind: "oauth.v1".to_string(),
            value: serde_json::json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
                "expires_at": expires_at,
                "endpoint": endpoint,
            }),
        },
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
