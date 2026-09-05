//! Persistent cached model catalog in ~/.rho/models-store.json.

use super::discovery::DiscoveredModel;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelStore {
    #[serde(skip)]
    file_path: PathBuf,
    pub updated_at_ms: i64,
    pub models: HashMap<String, Vec<DiscoveredModel>>,
}

impl ModelStore {
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if !path.exists() {
            return Self {
                file_path: path.to_path_buf(),
                updated_at_ms: 0,
                models: HashMap::new(),
            };
        }

        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(mut store) = serde_json::from_str::<Self>(&content)
        {
            store.file_path = path.to_path_buf();
            return store;
        }

        Self {
            file_path: path.to_path_buf(),
            updated_at_ms: 0,
            models: HashMap::new(),
        }
    }

    pub fn get_models(&self, provider: &str) -> Option<&Vec<DiscoveredModel>> {
        self.models.get(provider)
    }

    /// Providers that have a cached catalog on disk.
    pub fn providers(&self) -> impl Iterator<Item = &String> {
        self.models.keys()
    }

    /// Context window recorded for a model in the first matching catalog.
    pub fn context_tokens(&self, catalog_keys: &[&str], model: &str) -> Option<usize> {
        catalog_keys.iter().find_map(|key| {
            self.get_models(key)
                .and_then(|models| models.iter().find(|m| m.id == model))
                .and_then(|m| m.context_tokens)
        })
    }

    pub fn set_models(&mut self, provider: &str, models: Vec<DiscoveredModel>) -> Result<()> {
        self.models.insert(provider.to_string(), models);
        self.updated_at_ms = chrono::Utc::now().timestamp_millis();
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(anyhow::anyhow!("Failed to serialize model store: {e}")))?;

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
