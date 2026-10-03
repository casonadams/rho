use super::client::McpToolDefinition;
use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CACHE_FILE_NAME: &str = "mcp-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCacheEntry {
    pub definition_hash: String,
    pub tools: Vec<McpToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCache {
    #[serde(default)]
    pub servers: BTreeMap<String, McpCacheEntry>,
}

pub fn compute_definition_hash(config: &McpServerConfig) -> String {
    let mut hasher = Sha256::new();
    if let Some(cmd) = &config.command {
        hasher.update(b"command:");
        hasher.update(cmd.as_bytes());
        hasher.update(b"\n");
    }
    for arg in &config.args {
        hasher.update(b"arg:");
        hasher.update(arg.as_bytes());
        hasher.update(b"\n");
    }
    for (k, v) in &config.env {
        hasher.update(b"env:");
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.as_bytes());
        hasher.update(b"\n");
    }
    if let Some(url) = &config.url {
        hasher.update(b"url:");
        hasher.update(url.as_bytes());
        hasher.update(b"\n");
    }
    for (k, v) in &config.headers {
        hasher.update(b"header:");
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn default_cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CACHE_FILE_NAME)
}

impl McpCache {
    pub fn load_from_dir(config_dir: &Path) -> Self {
        let path = default_cache_path(config_dir);
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Self {
        if !path.is_file() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save_to_dir(&self, config_dir: &Path) -> Result<()> {
        let path = default_cache_path(config_dir);
        self.save_to_path(&path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize MCP cache: {e}")))?;

        let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, serialized)
            .map_err(|e| AppError::Config(format!("Failed to write MCP cache temporary file: {e}")))?;

        std::fs::rename(&tmp_path, path)
            .map_err(|e| AppError::Config(format!("Failed to atomically rename MCP cache file: {e}")))?;

        Ok(())
    }

    pub fn get_valid_tools(&self, server_name: &str, current_hash: &str) -> Option<&[McpToolDefinition]> {
        let entry = self.servers.get(server_name)?;
        if entry.definition_hash == current_hash {
            Some(&entry.tools)
        } else {
            None
        }
    }

    pub fn update(
        &mut self,
        server_name: &str,
        definition_hash: String,
        tools: Vec<McpToolDefinition>,
        capabilities: Option<Value>,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.servers.insert(
            server_name.to_string(),
            McpCacheEntry {
                definition_hash,
                tools,
                capabilities,
                updated_at: now,
            },
        );
    }

    pub fn invalidate(&mut self, server_name: &str) -> Option<McpCacheEntry> {
        self.servers.remove(server_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_definition_hash_changes_on_diff() {
        let cfg1 = McpServerConfig::stdio("echo", vec!["hello".to_string()]);
        let mut cfg2 = McpServerConfig::stdio("echo", vec!["world".to_string()]);

        let h1 = compute_definition_hash(&cfg1);
        let h2 = compute_definition_hash(&cfg2);
        assert_ne!(h1, h2);

        cfg2.args = vec!["hello".to_string()];
        assert_eq!(h1, compute_definition_hash(&cfg2));
    }

    #[test]
    fn test_cache_roundtrip_and_invalidation() {
        let temp = tempfile::tempdir().unwrap();
        let cache_file = temp.path().join("mcp-cache.json");

        let mut cache = McpCache::default();
        let cfg = McpServerConfig::stdio("test", vec!["1".to_string()]);
        let hash = compute_definition_hash(&cfg);

        let tool = McpToolDefinition {
            name: "t1".to_string(),
            description: Some("desc".to_string()),
            input_schema: serde_json::json!({}),
        };

        cache.update("srv1", hash.clone(), vec![tool], None);
        cache.save_to_path(&cache_file).unwrap();

        let loaded = McpCache::load_from_path(&cache_file);
        assert_eq!(loaded.get_valid_tools("srv1", &hash).unwrap().len(), 1);
        assert!(loaded.get_valid_tools("srv1", "different_hash").is_none());
        assert!(loaded.get_valid_tools("non_existent", &hash).is_none());
    }

    #[test]
    fn test_corrupted_cache_file_falls_back() {
        let temp = tempfile::tempdir().unwrap();
        let cache_file = temp.path().join("mcp-cache.json");
        std::fs::write(&cache_file, "INVALID JSON").unwrap();

        let loaded = McpCache::load_from_path(&cache_file);
        assert!(loaded.servers.is_empty());
    }
}
