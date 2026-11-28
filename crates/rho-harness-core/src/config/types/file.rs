use super::app::{ModelRole, SubagentsConfig};
use super::integrations::{PermissionConfig, ProviderConfig};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelsConfig {
    pub default: Option<String>,
    pub roles: BTreeMap<String, String>,
    /// Non-string or invalid keys, kept so loading can warn about them.
    pub extra: BTreeMap<String, toml::Value>,
}

impl ModelsConfig {
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.roles.is_empty() && self.extra.is_empty()
    }

    pub fn role(&self, role: &str) -> Option<&str> {
        self.roles.get(role).map(String::as_str)
    }

    pub fn set_role(&mut self, role: &ModelRole, value: Option<String>) {
        match value {
            Some(spec) => {
                self.roles.insert(role.key().to_string(), spec);
            }
            None => {
                self.roles.remove(role.key());
            }
        }
    }
}

impl Serialize for ModelsConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = usize::from(self.default.is_some()) + self.roles.len() + self.extra.len();
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(ref default) = self.default {
            map.serialize_entry("default", default)?;
        }
        for (role, spec) in &self.roles {
            map.serialize_entry(role, spec)?;
        }
        for (key, value) in &self.extra {
            if key != "default" && !self.roles.contains_key(key) {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ModelsConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
        let mut config = Self::default();
        for (key, value) in raw {
            if key == "default" {
                match value {
                    toml::Value::String(s) => config.default = Some(s),
                    other => {
                        config.extra.insert(key, other);
                    }
                }
                continue;
            }
            if let toml::Value::String(spec) = &value
                && let Ok(role) = ModelRole::parse(&key)
            {
                config.roles.insert(role.as_str().to_string(), spec.clone());
                continue;
            }
            config.extra.insert(key, value);
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct FileConfig {
    pub max_output_tokens: Option<u64>,
    pub max_turns: Option<usize>,
    pub context_limit: Option<usize>,
    pub context_window_messages: Option<usize>,
    pub compaction_max_bytes: Option<usize>,
    pub reserve_tokens: Option<usize>,
    pub keep_recent_tokens: Option<usize>,
    pub search_min_interval_ms: Option<u64>,
    pub search_timeout_sec: Option<u64>,
    pub fetch_timeout_sec: Option<u64>,
    pub fetch_limit: Option<usize>,
    pub fetch_max_bytes: Option<usize>,
    pub output_max_bytes: Option<usize>,
    pub allow_private_network: Option<bool>,
    pub region: Option<String>,
    pub show_label: Option<bool>,
    pub steering_mode: Option<crate::queue::QueueMode>,
    pub follow_up_mode: Option<crate::queue::QueueMode>,
    #[serde(
        default,
        alias = "thinking",
        alias = "default_thinking",
        alias = "default_thinking_level"
    )]
    pub thinking_level: Option<String>,
    pub cache_retention: Option<super::app::CacheRetention>,
    pub context_injection_max_tokens: Option<usize>,
    #[serde(default, alias = "retention_days")]
    pub session_retention_days: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub permission: Option<PermissionConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfigFile>,
    #[serde(default)]
    pub ui: Option<super::UiConfig>,
    #[serde(default, skip_serializing_if = "ModelsConfig::is_empty")]
    pub models: ModelsConfig,
    #[serde(default)]
    pub tools: Option<ToolsConfigFile>,
    #[serde(default, skip_serializing_if = "SubagentsConfig::is_empty")]
    pub subagents: SubagentsConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeConfigFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recap: Option<RecapConfigFile>,
    /// Top-level keys this version does not know (for example `model` and `provider` from v0.10).
    /// Saving a setting edits `config.toml` in place, so unknown keys survive at every level.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolsConfigFile {
    #[serde(default)]
    pub web: Option<WebToolsConfigFile>,
    #[serde(default)]
    pub lsp: Option<LspConfigFile>,
    #[serde(default)]
    pub bash: Option<BashConfigFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BashConfigFile {
    #[serde(default)]
    pub direnv: Option<bool>,
    #[serde(default)]
    pub direnv_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LspConfigFile {
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebToolsConfigFile {
    #[serde(default)]
    pub search: Option<WebSearchConfigFile>,
    #[serde(default)]
    pub fetch: Option<WebFetchConfigFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebSearchConfigFile {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub fallback: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebFetchConfigFile {
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpConfigFile {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "deferThreshold")]
    pub defer_threshold: Option<usize>,
    #[serde(default, alias = "startupTimeoutMs")]
    pub startup_timeout_ms: Option<u64>,
    #[serde(default)]
    pub servers: BTreeMap<String, super::integrations::McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorktreeConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean_source: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RecapConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_resume: Option<bool>,
}
