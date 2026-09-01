use rho_core::error::{AppError, Result};
use rho_core::provider::ProviderId;
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
    keys: HashMap<String, String>,
}

impl AuthStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self {
                file_path: path.to_path_buf(),
                keys: HashMap::new(),
            });
        }
        let content =
            std::fs::read_to_string(path).map_err(|e| AppError::Auth(format!("Failed to read auth file: {e}")))?;
        let keys: HashMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
        Ok(Self {
            file_path: path.to_path_buf(),
            keys,
        })
    }

    pub fn get_key(&self, provider: &str) -> Result<Option<String>> {
        if let Ok(id) = ProviderId::from_str(provider)
            && let Some(env_name) = id.api_key_env()
            && let Ok(val) = std::env::var(env_name)
            && !val.trim().is_empty()
        {
            return Ok(Some(val.trim().to_string()));
        }
        let generic_env = format!("{}_API_KEY", provider.to_ascii_uppercase().replace('-', "_"));
        if let Ok(val) = std::env::var(&generic_env)
            && !val.trim().is_empty()
        {
            return Ok(Some(val.trim().to_string()));
        }
        Ok(self.keys.get(provider).cloned())
    }

    pub fn set_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.keys.insert(provider.to_string(), key.into());
        self.save()
    }

    pub fn set_api_key(&mut self, provider: &str, key: impl Into<String>) -> Result<()> {
        self.set_key(provider, key)
    }

    pub fn remove_key(&mut self, provider: &str) -> Result<()> {
        self.keys.remove(provider);
        self.save()
    }

    pub fn list_configured_providers(&self) -> Vec<String> {
        let mut list: Vec<String> = self.keys.keys().cloned().collect();
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
        self.keys.values().cloned().collect()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&self.keys)
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
