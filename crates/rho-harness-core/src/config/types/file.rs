use super::integrations::{PermissionConfig, ProviderConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisor: Option<String>,
}

impl ModelsConfig {
    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.guard.is_none() && self.plan.is_none() && self.advisor.is_none()
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolsConfigFile {
    #[serde(default)]
    pub web: Option<WebToolsConfigFile>,
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
    #[serde(default)]
    pub multimodal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpConfigFile {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "deferThreshold")]
    pub defer_threshold: Option<usize>,
    #[serde(default)]
    pub servers: BTreeMap<String, super::integrations::McpServerConfig>,
}
