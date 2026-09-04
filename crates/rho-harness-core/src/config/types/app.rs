use super::integrations::{McpConfig, PluginConfig, ProviderConfig};
use super::paths::default_config_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_MAX_TURNS: usize = 250;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub max_output_tokens: Option<u64>,
    pub max_turns: usize,
    pub context_limit: Option<usize>,
    pub context_window_messages: usize,
    pub compaction_max_bytes: usize,
    pub reserve_tokens: usize,
    pub keep_recent_tokens: usize,
    pub search_min_interval_ms: u64,
    pub search_timeout_sec: u64,
    pub fetch_timeout_sec: u64,
    pub fetch_limit: usize,
    pub fetch_max_bytes: usize,
    pub output_max_bytes: usize,
    pub allow_private_network: bool,
    pub region: String,
    pub show_label: bool,
    pub steering_mode: crate::queue::QueueMode,
    pub follow_up_mode: crate::queue::QueueMode,
    pub thinking_level: Option<String>,
    pub context_injection_max_tokens: usize,
    pub plugins: BTreeMap<String, PluginConfig>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub mcp: McpConfig,
    pub config_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub auth_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let base_dir = default_config_dir();
        Self {
            model: "claude-3-7-sonnet-20250219".to_string(),
            provider: "anthropic".to_string(),
            auto_approve: false,
            max_output_tokens: None,
            max_turns: DEFAULT_MAX_TURNS,
            context_limit: None,
            context_window_messages: crate::session::context::DEFAULT_CONTEXT_WINDOW_MESSAGES,
            compaction_max_bytes: crate::session::context::DEFAULT_COMPACTION_MAX_BYTES,
            reserve_tokens: crate::tokens::DEFAULT_RESERVE_TOKENS,
            keep_recent_tokens: crate::tokens::DEFAULT_KEEP_RECENT_TOKENS,
            search_min_interval_ms: 2000,
            search_timeout_sec: 12,
            fetch_timeout_sec: 8,
            fetch_limit: 200,
            fetch_max_bytes: 5_000_000,
            output_max_bytes: 50_000,
            allow_private_network: false,
            region: "wt-wt".to_string(),
            show_label: false,
            steering_mode: crate::queue::QueueMode::OneAtATime,
            follow_up_mode: crate::queue::QueueMode::OneAtATime,
            thinking_level: None,
            context_injection_max_tokens: 4000,
            plugins: BTreeMap::new(),
            providers: BTreeMap::new(),
            mcp: McpConfig::default(),
            sessions_dir: base_dir.join("sessions"),
            auth_file: base_dir.join("auth.json"),
            config_dir: base_dir,
        }
    }
}
