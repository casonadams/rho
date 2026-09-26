use super::integrations::{PermissionConfig, ProviderConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct FileConfig {
    #[serde(default, alias = "default_model")]
    pub model: Option<String>,
    #[serde(default, alias = "default_provider")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
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
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub permission: Option<PermissionConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfigFile>,
    #[serde(default)]
    pub ui: Option<super::UiConfig>,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
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
