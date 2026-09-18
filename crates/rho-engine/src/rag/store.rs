use super::chunker::CodeChunk;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseIndex {
    pub version: u32,
    pub model: String,
    pub chunks: Vec<CodeChunk>,
}

impl Default for CodebaseIndex {
    fn default() -> Self {
        Self {
            version: 1,
            model: "bge-small-en-v1.5".to_string(),
            chunks: Vec::new(),
        }
    }
}

impl CodebaseIndex {
    pub fn index_path(base_dir: &Path) -> PathBuf {
        base_dir.join(".rho").join("index.db")
    }

    pub fn load(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let data = std::fs::read(path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    pub async fn load_async(path: &Path) -> Option<Self> {
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            return None;
        }
        let data = tokio::fs::read(path).await.ok()?;
        serde_json::from_slice(&data).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn save_async(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        tokio::fs::write(&tmp, data).await.map_err(|e| e.to_string())?;
        tokio::fs::rename(tmp, path).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
