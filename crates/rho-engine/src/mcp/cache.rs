use super::client::McpToolDefinition;
use crate::digest::sha256_hex;
use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
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
    let mut bytes = Vec::new();
    if let Some(cmd) = &config.command {
        bytes.extend_from_slice(b"command:");
        bytes.extend_from_slice(cmd.as_bytes());
        bytes.extend_from_slice(b"\n");
    }
    for arg in &config.args {
        bytes.extend_from_slice(b"arg:");
        bytes.extend_from_slice(arg.as_bytes());
        bytes.extend_from_slice(b"\n");
    }
    for (k, v) in &config.env {
        bytes.extend_from_slice(b"env:");
        bytes.extend_from_slice(k.as_bytes());
        bytes.extend_from_slice(b"=");
        bytes.extend_from_slice(v.as_bytes());
        bytes.extend_from_slice(b"\n");
    }
    if let Some(url) = &config.url {
        bytes.extend_from_slice(b"url:");
        bytes.extend_from_slice(url.as_bytes());
        bytes.extend_from_slice(b"\n");
    }
    for (k, v) in &config.headers {
        bytes.extend_from_slice(b"header:");
        bytes.extend_from_slice(k.as_bytes());
        bytes.extend_from_slice(b"=");
        bytes.extend_from_slice(v.as_bytes());
        bytes.extend_from_slice(b"\n");
    }
    update_with_json(&mut bytes, "transport", &config.transport);
    update_with_json(&mut bytes, "oauth", &config.oauth);
    update_with_json(&mut bytes, "mode", &config.mode);
    update_with_json(&mut bytes, "include_tools", &config.include_tools);
    update_with_json(&mut bytes, "exclude_tools", &config.exclude_tools);
    update_with_json(&mut bytes, "direct_tools", &config.direct_tools);
    update_with_json(&mut bytes, "instructions", &config.instructions);
    sha256_hex(bytes)
}

/// Unset fields add nothing, so approvals of configs that predate a field stay valid.
fn update_with_json<T: Serialize>(bytes: &mut Vec<u8>, label: &str, value: &Option<T>) {
    if let Some(value) = value {
        bytes.extend_from_slice(label.as_bytes());
        bytes.extend_from_slice(b":");
        bytes.extend_from_slice(&serde_json::to_vec(value).unwrap_or_default());
        bytes.extend_from_slice(b"\n");
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
        let serialized = serde_json::to_vec_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize MCP cache: {e}")))?;
        rho_harness_core::fs::write_atomic(path, &serialized, 0o600)
            .map_err(|e| AppError::Config(format!("Failed to save MCP cache: {e}")))
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
        let now = crate::clock::unix_now_secs();

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

        let expected = sha256_hex(b"command:srv\n");
        assert_eq!(compute_definition_hash(&base), expected);
    }

    #[test]
    fn test_cache_roundtrip_and_invalidation() {
        let temp = crate::test_support::TestDir::new();
        let cache_file = temp.join("mcp-cache.json");

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
    fn saving_leaves_only_the_cache_file_and_fails_when_the_directory_is_blocked() {
        let dir = rho_test_support::TestDir::new();
        let path = dir.join("mcp").join(CACHE_FILE_NAME);
        McpCache::default().save_to_path(&path).unwrap();
        assert_eq!(McpCache::load_from_path(&path), McpCache::default());
        let names: Vec<_> = std::fs::read_dir(dir.join("mcp"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [CACHE_FILE_NAME]);

        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "file").unwrap();
        assert!(
            McpCache::default()
                .save_to_path(&blocker.join(CACHE_FILE_NAME))
                .is_err()
        );
    }

    #[test]
    fn test_corrupted_cache_file_falls_back() {
        let temp = crate::test_support::TestDir::new();
        let cache_file = temp.join("mcp-cache.json");
        std::fs::write(&cache_file, "INVALID JSON").unwrap();

        let loaded = McpCache::load_from_path(&cache_file);
        assert!(loaded.servers.is_empty());
    }
}
