use super::client::McpToolDefinition;
use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CACHE_FILE_NAME: &str = "mcp-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCacheEntry {
    pub definition_hash: String,
    pub tools: Vec<McpToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
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
    update_with_json(&mut hasher, "transport", &config.transport);
    update_with_json(&mut hasher, "oauth", &config.oauth);
    update_with_json(&mut hasher, "mode", &config.mode);
    update_with_json(&mut hasher, "include_tools", &config.include_tools);
    update_with_json(&mut hasher, "exclude_tools", &config.exclude_tools);
    update_with_json(&mut hasher, "direct_tools", &config.direct_tools);
    update_with_json(&mut hasher, "instructions", &config.instructions);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Unset fields add nothing, so approvals of configs that predate a field stay valid.
fn update_with_json<T: Serialize>(hasher: &mut Sha256, label: &str, value: &Option<T>) {
    if let Some(value) = value {
        hasher.update(label.as_bytes());
        hasher.update(b":");
        hasher.update(serde_json::to_vec(value).unwrap_or_default());
        hasher.update(b"\n");
    }
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

    pub fn get_valid(&self, server_name: &str, current_hash: &str) -> Option<&McpCacheEntry> {
        self.servers
            .get(server_name)
            .filter(|entry| entry.definition_hash == current_hash)
    }

    pub fn update(
        &mut self,
        server_name: &str,
        definition_hash: String,
        tools: Vec<McpToolDefinition>,
        instructions: Option<String>,
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
                instructions,
                updated_at: now,
            },
        );
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
    fn approval_hash_covers_oauth_transport_exposure_and_tool_filters() {
        use rho_harness_core::config::{McpDirectTools, McpExposureMode, McpOAuthConfig, McpTransportKind};
        let base = McpServerConfig {
            command: Some("srv".into()),
            ..Default::default()
        };
        let base_hash = compute_definition_hash(&base);
        let changes: Vec<(&str, McpServerConfig)> = vec![
            (
                "transport",
                McpServerConfig {
                    transport: Some(McpTransportKind::Stdio),
                    ..base.clone()
                },
            ),
            (
                "oauth",
                McpServerConfig {
                    oauth: Some(McpOAuthConfig {
                        client_id: Some("x".into()),
                        ..Default::default()
                    }),
                    ..base.clone()
                },
            ),
            (
                "mode",
                McpServerConfig {
                    mode: Some(McpExposureMode::Direct),
                    ..base.clone()
                },
            ),
            (
                "include_tools",
                McpServerConfig {
                    include_tools: Some(vec!["a".into()]),
                    ..base.clone()
                },
            ),
            (
                "exclude_tools",
                McpServerConfig {
                    exclude_tools: Some(vec!["a".into()]),
                    ..base.clone()
                },
            ),
            (
                "direct_tools",
                McpServerConfig {
                    direct_tools: Some(McpDirectTools::All(true)),
                    ..base.clone()
                },
            ),
            (
                "instructions",
                McpServerConfig {
                    instructions: Some(true),
                    ..base.clone()
                },
            ),
        ];
        for (field, changed) in changes {
            assert_ne!(compute_definition_hash(&changed), base_hash, "{field}");
        }
        let mut a = base.clone();
        a.oauth = Some(McpOAuthConfig {
            scopes: vec!["read".into()],
            ..Default::default()
        });
        let mut b = a.clone();
        b.oauth.as_mut().unwrap().scopes = vec!["admin".into()];
        assert_ne!(compute_definition_hash(&a), compute_definition_hash(&b));
    }

    #[test]
    fn approval_hash_ignores_operational_settings_and_keeps_legacy_values() {
        let base = McpServerConfig {
            command: Some("srv".into()),
            ..Default::default()
        };
        let tuned = McpServerConfig {
            timeout_seconds: Some(5),
            idle_timeout_seconds: Some(9),
            enabled: false,
            ..base.clone()
        };
        assert_eq!(compute_definition_hash(&base), compute_definition_hash(&tuned));

        let mut legacy = Sha256::new();
        legacy.update(b"command:srv\n");
        let expected: String = legacy.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(compute_definition_hash(&base), expected);
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
        assert_eq!(loaded.get_valid("srv1", &hash).unwrap().tools.len(), 1);
        assert!(loaded.get_valid("srv1", "different_hash").is_none());
        assert!(loaded.get_valid("non_existent", &hash).is_none());
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
