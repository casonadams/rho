//! Secure persistent credential store supporting API keys and OAuth tokens.

mod io;

use super::oauth::refresh_oauth_token;
use super::resolver::{resolve_secret_value, resolve_secret_value_async};
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::provider::{Connection, ProviderId};
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
            return resolve_env_key_async(provider).await;
        };
        match cred {
            StoredCredential::ApiKey { key, .. } => resolve_secret_value_async(key).await.map(Some),
            StoredCredential::OAuth { .. } if force_refresh || cred.is_expired(60) => {
                self.refresh_stored(provider).await.map(Some)
            }
            StoredCredential::OAuth { .. } => Ok(Some(cred.raw_secret().to_string())),
        }
    }

    /// Adopts an OAuth credential another process saved to disk when it is at least as fresh as
    /// memory, since refresh tokens may be single-use. Returns whether memory changed.
    async fn adopt_newer_credential_from_disk(&mut self, provider: &str) -> Result<bool> {
        let mut on_disk = io::load_credentials_async(&self.file_path).await?;
        match on_disk.remove(provider) {
            Some(disk @ StoredCredential::OAuth { .. }) if is_fresher(&disk, self.credentials.get(provider)) => {
                self.credentials.insert(provider.to_string(), disk);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn refresh_stored(&mut self, provider: &str) -> Result<String> {
        let lock = io::lock_auth_file_async(&self.file_path).await?;
        if self.adopt_newer_credential_from_disk(provider).await?
            && let Some(cred) = self.credentials.get(provider)
            && !cred.is_expired(60)
        {
            return Ok(cred.raw_secret().to_string());
        }
        let provider_id = Connection::parse(provider)
            .provider_id()
            .ok_or_else(|| AppError::Auth(format!("Unknown provider '{provider}'")))?;
        let cred = self
            .credentials
            .get(provider)
            .ok_or_else(|| AppError::Auth(format!("No credential found for provider '{provider}'")))?;
        let refreshed = refresh_oauth_token(provider_id, cred).await?;
        let access = refreshed.raw_secret().to_string();
        self.credentials.insert(provider.to_string(), refreshed.clone());
        io::write_entry_locked_async(&lock, &self.file_path, provider, refreshed).await?;
        Ok(access)
    }

    pub async fn force_refresh(&mut self, provider: &str) -> Result<Option<String>> {
        self.resolve_key(provider, true).await
    }

    pub fn get_credential(&self, provider: &str) -> Option<&StoredCredential> {
        self.credentials.get(provider)
    }

    pub fn set_credential(&mut self, provider: &str, cred: StoredCredential) -> Result<()> {
        self.credentials.insert(provider.to_string(), cred.clone());
        io::write_entry(&self.file_path, provider, Some(&cred))
    }

    pub async fn set_credential_async(&mut self, provider: &str, cred: StoredCredential) -> Result<()> {
        self.credentials.insert(provider.to_string(), cred.clone());
        io::write_entry_async(&self.file_path, provider, Some(cred)).await
    }

    pub fn set_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_credential(provider, StoredCredential::api_key(key.into()))
    }

    pub async fn set_key_async(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_credential_async(provider, StoredCredential::api_key(key.into()))
            .await
    }

    pub fn remove_key(&mut self, provider: &str) -> Result<()> {
        self.credentials.remove(provider);
        io::write_entry(&self.file_path, provider, None)
    }

    pub fn list_stored_providers(&self) -> Vec<String> {
        let mut list: Vec<String> = self.credentials.keys().cloned().collect();
        list.sort();
        list
    }

    /// Every stored login, default logins and additional accounts alike.
    pub fn stored_logins(&self) -> Vec<Connection> {
        let mut logins: Vec<Connection> = self.credentials.keys().map(|key| Connection::parse(key)).collect();
        logins.sort_by_key(ToString::to_string);
        logins
    }

    /// Stored logins to `provider`, default first.
    pub fn logins(&self, provider: ProviderId) -> Vec<Login> {
        let mut logins: Vec<Login> = self
            .credentials
            .iter()
            .map(|(key, cred)| Login {
                connection: Connection::parse(key),
                email: account_email(cred).map(str::to_string),
            })
            .filter(|login| login.connection.provider == provider.as_str())
            .collect();
        logins.sort_by_key(|login| (login.connection.account.is_some(), login.connection.to_string()));
        logins
    }

    /// Where a new login is saved: the login already holding the same account, else the default
    /// key when it is free, else a `provider:email` key beside the existing default login.
    pub fn login_key(&self, provider: ProviderId, cred: &StoredCredential) -> Connection {
        let default = Connection::parse(provider.as_str());
        let Some(email) = account_email(cred) else {
            return default;
        };
        let logins = self.logins(provider);
        let same_account = logins
            .iter()
            .find(|login| login.email.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(email)));
        if let Some(login) = same_account {
            return login.connection.clone();
        }
        if logins.iter().any(|login| login.connection == default) {
            Connection::account(provider, email)
        } else {
            default
        }
    }

    pub fn list_configured_providers(&self) -> Vec<String> {
        let mut list: Vec<String> = self.credentials.keys().cloned().collect();
        for id in ProviderId::ALL {
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
}

/// One stored login and the account email its credential carries, when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub connection: Connection,
    pub email: Option<String>,
}

impl AuthStore {
    /// The account id an OAuth login stores: the ChatGPT account or the Antigravity project.
    pub fn account_id(&self, connection: &str) -> Option<String> {
        match self.get_credential(connection) {
            Some(StoredCredential::OAuth {
                account_id: Some(id), ..
            }) => Some(id.clone()),
            _ => None,
        }
    }

    /// The login's ChatGPT account: stored on the login, else read from `token`'s claims.
    pub fn chatgpt_account_id(&self, connection: &str, token: &str) -> Option<String> {
        self.account_id(connection)
            .or_else(|| crate::auth::oauth::extract_chatgpt_account_id(token))
    }

    /// The login's Cloud Code project, else the project shared by logins without one.
    pub fn antigravity_project(&self, connection: &str) -> String {
        self.account_id(connection)
            .unwrap_or_else(crate::auth::antigravity::default_project_id)
    }
}

fn account_email(cred: &StoredCredential) -> Option<&str> {
    match cred {
        StoredCredential::OAuth {
            account_email: Some(email),
            ..
        } => Some(email.as_str()),
        StoredCredential::OAuth { .. } | StoredCredential::ApiKey { .. } => None,
    }
}

fn expiry_ms(cred: &StoredCredential) -> i64 {
    match cred {
        StoredCredential::OAuth {
            expires_at_ms: Some(exp),
            ..
        } => *exp,
        _ => i64::MIN,
    }
}

fn is_fresher(disk: &StoredCredential, memory: Option<&StoredCredential>) -> bool {
    memory.is_none_or(|memory| memory != disk && expiry_ms(disk) >= expiry_ms(memory))
}

fn env_key_value(provider: &str) -> Option<String> {
    if let Ok(id) = ProviderId::from_str(provider)
        && let Some(env_name) = id.api_key_env()
        && let Some(val) = non_blank_env(env_name)
    {
        return Some(val);
    }
    let generic_env = format!("{}_API_KEY", provider.to_ascii_uppercase().replace('-', "_"));
    non_blank_env(&generic_env)
}

fn non_blank_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|val| !val.trim().is_empty())
}

fn resolve_env_key(provider: &str) -> Result<Option<String>> {
    match env_key_value(provider) {
        Some(val) => resolve_secret_value(&val).map(Some),
        None => Ok(None),
    }
}

async fn resolve_env_key_async(provider: &str) -> Result<Option<String>> {
    match env_key_value(provider) {
        Some(val) => resolve_secret_value_async(&val).await.map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn env_secret_command_does_not_block_the_runtime() {
        let provider = "rho-test-async-env";
        unsafe { std::env::set_var("RHO_TEST_ASYNC_ENV_API_KEY", "!echo async_env_secret") };
        let mut store = AuthStore::default();
        let done = Cell::new(false);
        let ticks = Cell::new(0u32);
        let resolve = async {
            let key = store.get_key(provider).await;
            done.set(true);
            key
        };
        let ticker = async {
            while !done.get() {
                ticks.set(ticks.get() + 1);
                tokio::task::yield_now().await;
            }
        };
        let (key, ()) = tokio::join!(resolve, ticker);
        unsafe { std::env::remove_var("RHO_TEST_ASYNC_ENV_API_KEY") };
        assert_eq!(key.unwrap().as_deref(), Some("async_env_secret"));
        assert!(ticks.get() > 0);
    }
}
