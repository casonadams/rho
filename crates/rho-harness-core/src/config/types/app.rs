use super::integrations::{McpConfig, PermissionConfig, ProviderConfig, default_true};
use super::paths::default_config_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_MAX_TURNS: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model: String,
    pub provider: String,
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
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub append_system_prompt: Option<String>,
    #[serde(default)]
    pub no_context_files: bool,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    #[serde(default)]
    pub session_retention_days: Option<u32>,
    #[serde(skip)]
    pub migration_warnings: Vec<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub mcp: McpConfig,
    pub permission: PermissionConfig,
    pub ui: super::UiConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    pub config_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub auth_file: PathBuf,
}

macro_rules! default_config_literal {
    ($base_dir:expr) => {
        Config {
            model: "llama3.2".to_string(),
            provider: "local".to_string(),
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
            system_prompt: None,
            append_system_prompt: None,
            no_context_files: false,
            default_model: None,
            default_provider: None,
            models: BTreeMap::new(),
            session_retention_days: Some(5),
            migration_warnings: Vec::new(),
            providers: BTreeMap::new(),
            mcp: McpConfig::default(),
            permission: PermissionConfig::default(),
            ui: $crate::config::UiConfig::default(),
            tools: $crate::config::ToolsConfig::default(),
            sessions_dir: $base_dir.join("sessions"),
            auth_file: $base_dir.join("auth.json"),
            config_dir: $base_dir,
        }
    };
}

impl Default for Config {
    fn default() -> Self {
        let base_dir = default_config_dir();
        default_config_literal!(base_dir)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebToolsConfig {
    #[serde(default)]
    pub search: WebSearchConfig,
    #[serde(default)]
    pub fetch: WebFetchConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFetchConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub multimodal: bool,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            multimodal: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_search_provider")]
    pub default: String,
    #[serde(default = "default_search_fallback")]
    pub fallback: Vec<String>,
}

fn default_search_provider() -> String {
    "brave".to_string()
}

fn default_search_fallback() -> Vec<String> {
    vec!["duckduckgo".to_string(), "yahoo".to_string()]
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default: default_search_provider(),
            fallback: default_search_fallback(),
        }
    }
}

impl Config {
    pub fn set_default_model(&mut self, model: &str, provider: &str) {
        self.model = model.to_string();
        self.provider = provider.to_string();
        self.default_model = Some(model.to_string());
        self.default_provider = Some(provider.to_string());
        self.models.insert(provider.to_string(), model.to_string());
    }

    pub fn guard_model(&self) -> Option<&str> {
        self.models
            .get("guard")
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
    }

    pub fn set_guard_model(&mut self, guard: Option<&str>) {
        match guard
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
        {
            Some(g) => {
                self.models.insert("guard".to_string(), g.to_string());
            }
            None => {
                self.models.remove("guard");
            }
        }
    }

    pub fn canonical_model_spec(&self) -> String {
        let trimmed = self.model.trim();
        if trimmed.is_empty() {
            let provider = if self.provider.trim().is_empty() {
                "local"
            } else {
                self.provider.trim()
            };
            let default_m = crate::provider::default_model_for_provider(provider);
            return format!("{provider}/{default_m}");
        }
        if let Some((p, m)) = trimmed.split_once('/') {
            let p = p.trim();
            let m = m.trim();
            if !p.is_empty() {
                return format!("{p}/{m}");
            }
        }
        let provider = if !self.provider.trim().is_empty() {
            self.provider.trim()
        } else {
            crate::provider::infer_provider_for_model(trimmed).unwrap_or("local")
        };
        format!("{provider}/{trimmed}")
    }
}
