use super::integrations::{McpConfig, PermissionConfig, PluginConfig, ProviderConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct FileConfig {
    #[serde(default, alias = "default_model")]
    pub model: Option<String>,
    #[serde(default, alias = "default_provider")]
    pub provider: Option<String>,
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
    pub theme: Option<String>,
    pub context_injection_max_tokens: Option<usize>,
    #[serde(default, alias = "retention_days")]
    pub session_retention_days: Option<u32>,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
    #[serde(default)]
    pub permission: Option<PermissionConfig>,
}
