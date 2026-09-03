use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigKey {
    Model,
    Provider,
    AutoApprove,
    MaxOutputTokens,
    MaxTurns,
    ContextLimit,
    ContextWindowMessages,
    CompactionMaxBytes,
    SearchMinIntervalMs,
    SearchTimeoutSec,
    FetchTimeoutSec,
    FetchLimit,
    FetchMaxBytes,
    OutputMaxBytes,
    AllowPrivateNetwork,
    Region,
    SteeringMode,
    FollowUpMode,
    ReserveTokens,
    KeepRecentTokens,
    ThinkingLevel,
    DisableBuiltInSkills,
}

impl FromStr for ConfigKey {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "model" => Ok(Self::Model),
            "provider" => Ok(Self::Provider),
            "thinking_level" => Ok(Self::ThinkingLevel),
            "auto_approve" => Ok(Self::AutoApprove),
            "max_output_tokens" => Ok(Self::MaxOutputTokens),
            "max_turns" => Ok(Self::MaxTurns),
            "context_limit" => Ok(Self::ContextLimit),
            "context_window_messages" => Ok(Self::ContextWindowMessages),
            "compaction_max_bytes" => Ok(Self::CompactionMaxBytes),
            "search_min_interval_ms" => Ok(Self::SearchMinIntervalMs),
            "search_timeout_sec" => Ok(Self::SearchTimeoutSec),
            "fetch_timeout_sec" => Ok(Self::FetchTimeoutSec),
            "fetch_limit" => Ok(Self::FetchLimit),
            "fetch_max_bytes" => Ok(Self::FetchMaxBytes),
            "output_max_bytes" => Ok(Self::OutputMaxBytes),
            "allow_private_network" => Ok(Self::AllowPrivateNetwork),
            "region" => Ok(Self::Region),
            "steering_mode" => Ok(Self::SteeringMode),
            "follow_up_mode" => Ok(Self::FollowUpMode),
            "reserve_tokens" => Ok(Self::ReserveTokens),
            "keep_recent_tokens" => Ok(Self::KeepRecentTokens),
            "disable_built_in_skills" => Ok(Self::DisableBuiltInSkills),
            _ => Err(format!("unknown configuration key: {value}")),
        }
    }
}

impl ConfigKey {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Provider => "provider",
            Self::AutoApprove => "auto_approve",
            Self::MaxOutputTokens => "max_output_tokens",
            Self::MaxTurns => "max_turns",
            Self::ContextLimit => "context_limit",
            Self::ContextWindowMessages => "context_window_messages",
            Self::CompactionMaxBytes => "compaction_max_bytes",
            Self::SearchMinIntervalMs => "search_min_interval_ms",
            Self::SearchTimeoutSec => "search_timeout_sec",
            Self::FetchTimeoutSec => "fetch_timeout_sec",
            Self::FetchLimit => "fetch_limit",
            Self::FetchMaxBytes => "fetch_max_bytes",
            Self::OutputMaxBytes => "output_max_bytes",
            Self::AllowPrivateNetwork => "allow_private_network",
            Self::Region => "region",
            Self::SteeringMode => "steering_mode",
            Self::FollowUpMode => "follow_up_mode",
            Self::ReserveTokens => "reserve_tokens",
            Self::KeepRecentTokens => "keep_recent_tokens",
            Self::ThinkingLevel => "thinking_level",
            Self::DisableBuiltInSkills => "disable_built_in_skills",
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub replaces: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            command: None,
            args: Vec::new(),
            package: None,
            version: None,
            git: None,
            branch: None,
            tag: None,
            enabled: true,
            replaces: BTreeSet::new(),
            config: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
}

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
    pub disable_built_in_skills: bool,
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
            max_turns: 100,
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
            disable_built_in_skills: false,
            plugins: BTreeMap::new(),
            providers: BTreeMap::new(),
            mcp: McpConfig::default(),
            sessions_dir: base_dir.join("sessions"),
            auth_file: base_dir.join("auth.json"),
            config_dir: base_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct FileConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub auto_approve: Option<bool>,
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
    pub thinking_level: Option<String>,
    pub context_injection_max_tokens: Option<usize>,
    pub disable_built_in_skills: Option<bool>,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
}

pub fn default_config_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RHO_HOME") {
        return PathBuf::from(custom);
    }
    if let Ok(custom) = std::env::var("RUST_AI_HOME") {
        return PathBuf::from(custom);
    }
    let home = dirs_fallback();
    home.join(".config").join("rho")
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
