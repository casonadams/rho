use super::integrations::{McpConfig, PermissionConfig, ProviderConfig, default_true};
use super::paths::default_config_dir;
use crate::provider::ModelRoute;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_MAX_TURNS: usize = 1000;

/// A model chosen for a job other than the main conversation, set under `[models]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelSlot {
    Smol,
    Slow,
    Guard,
}

impl ModelSlot {
    pub const ALL: [Self; 3] = [Self::Smol, Self::Slow, Self::Guard];

    /// The `[models]` key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Smol => "smol",
            Self::Slow => "slow",
            Self::Guard => "guard",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Smol => "Smol",
            Self::Slow => "Slow",
            Self::Guard => "Guard",
        }
    }

    pub fn from_title(title: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.title() == title)
    }
}

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
    #[serde(default)]
    pub cache_retention: CacheRetention,
    pub context_injection_max_tokens: usize,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub append_system_prompt: Option<String>,
    #[serde(default)]
    pub no_context_files: bool,
    /// The saved default model as a canonical `connection/model` spec.
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    #[serde(default)]
    pub session_retention_days: Option<u32>,
    /// Problems found while loading config; shown to the user and otherwise ignored.
    #[serde(skip)]
    pub config_warnings: Vec<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub mcp: McpConfig,
    pub permission: PermissionConfig,
    pub ui: super::UiConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub subagents: SubagentsConfig,
    #[serde(default)]
    pub worktree: WorktreeConfig,
    #[serde(default)]
    pub hook_dirs: Vec<PathBuf>,
    pub config_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub auth_file: PathBuf,
}

macro_rules! default_config_literal {
    ($base_dir:expr) => {
        Config {
            model: String::new(),
            provider: String::new(),
            max_output_tokens: None,
            max_turns: DEFAULT_MAX_TURNS,
            context_limit: None,
            context_window_messages: crate::session::DEFAULT_CONTEXT_WINDOW_MESSAGES,
            compaction_max_bytes: crate::session::DEFAULT_COMPACTION_MAX_BYTES,
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
            thinking_level: crate::thinking::ThinkingLevel::Medium.setting(),
            cache_retention: CacheRetention::Auto,
            context_injection_max_tokens: 4000,
            system_prompt: None,
            append_system_prompt: None,
            no_context_files: false,
            default_model: None,
            models: BTreeMap::new(),
            session_retention_days: Some(5),
            config_warnings: Vec::new(),
            providers: BTreeMap::new(),
            mcp: McpConfig::default(),
            permission: PermissionConfig::default(),
            ui: $crate::config::UiConfig::default(),
            tools: $crate::config::ToolsConfig::default(),
            subagents: SubagentsConfig::default(),
            worktree: WorktreeConfig::default(),
            hook_dirs: Vec::new(),
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
pub struct WorktreeConfig {
    /// Directory under which managed session worktrees are created (default: `<config_dir>/wt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<PathBuf>,
    /// Whether `/wt` resets and cleans the source checkout after carrying changes into the worktree.
    #[serde(default)]
    pub clean_source: bool,
}

impl WorktreeConfig {
    pub fn is_empty(&self) -> bool {
        self.base.is_none() && !self.clean_source
    }

    pub fn effective_base(&self, config_dir: &std::path::Path) -> PathBuf {
        self.base.clone().unwrap_or_else(|| config_dir.join("wt"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub bash: BashConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashConfig {
    #[serde(default)]
    pub direnv: bool,
    #[serde(default = "default_direnv_timeout_ms")]
    pub direnv_timeout_ms: u64,
}

fn default_direnv_timeout_ms() -> u64 {
    30_000
}

impl Default for BashConfig {
    fn default() -> Self {
        Self {
            direnv: false,
            direnv_timeout_ms: default_direnv_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentsConfig {
    /// Model role per built-in role or custom agent name (lowercase), used when the caller passes none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, crate::workflow::ModelRole>,
    /// Seconds a subagent call waits for its result before the subagent moves to the background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_seconds: Option<u64>,
    /// What happens when an agent you are not viewing needs a permission answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_focus: Option<ApprovalFocus>,
}

/// Where the view goes when an agent other than the one shown asks for a permission answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalFocus {
    /// Switch the view to the asking agent and show its prompt.
    #[default]
    Switch,
    /// Keep the current view; the footer names the agent, and its prompt opens when you view it.
    Indicate,
}

impl SubagentsConfig {
    pub fn is_empty(&self) -> bool {
        self.models.is_empty() && self.foreground_seconds.is_none() && self.approval_focus.is_none()
    }
}

/// How long the provider keeps the prompt cache written by each request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheRetention {
    /// One hour on Claude subscription logins, five minutes everywhere else.
    #[default]
    Auto,
    /// Five minutes: the cheapest cache write.
    Short,
    /// One hour: costs more to write, but survives longer pauses between prompts.
    Long,
}

impl CacheRetention {
    /// Resolves `Auto` for a subscription login or a pay-per-token connection.
    pub fn resolve(self, subscription: bool) -> Self {
        match self {
            Self::Auto if subscription => Self::Long,
            Self::Auto => Self::Short,
            explicit => explicit,
        }
    }

    /// `Auto` becomes `Short` for work that ends within minutes, where a 1-hour write is never read back.
    pub fn for_short_lived_work(self) -> Self {
        match self {
            Self::Auto => Self::Short,
            explicit => explicit,
        }
    }

    /// Anthropic prompt-cache lifetimes for the default and `ttl: "1h"` breakpoints.
    pub fn ttl(self, subscription: bool) -> std::time::Duration {
        match self.resolve(subscription) {
            Self::Long => std::time::Duration::from_secs(60 * 60),
            _ => std::time::Duration::from_secs(5 * 60),
        }
    }
}

impl std::str::FromStr for CacheRetention {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "short" | "5m" => Ok(Self::Short),
            "long" | "1h" => Ok(Self::Long),
            _ => Err(format!(
                "invalid cache retention: '{value}', expected 'auto', 'short' or 'long'"
            )),
        }
    }
}

impl std::fmt::Display for CacheRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Short => "short",
            Self::Long => "long",
        })
    }
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
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self { enabled: true }
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
    /// Makes `provider`/`model` both the active selection and the saved default.
    pub fn set_default_model(&mut self, model: &str, provider: &str) {
        self.set_route(ModelRoute::new(provider, model));
        self.default_model = Some(self.canonical_model_spec());
    }

    fn resolve_named_model(&self, name: &str) -> Option<&str> {
        self.models
            .get(name)
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
    }

    /// The model configured for `slot`; unset, blank and `none` all mean no model.
    pub fn slot_model(&self, slot: ModelSlot) -> Option<&str> {
        self.resolve_named_model(slot.key())
    }

    pub fn guard_model(&self) -> Option<&str> {
        self.slot_model(ModelSlot::Guard)
    }

    /// The active selection; `model` may still carry a `provider/` prefix from older writers.
    pub fn route(&self) -> ModelRoute {
        match self.provider.trim() {
            "" => ModelRoute::parse(&self.model, None, "local"),
            provider => ModelRoute::new(provider, &self.model),
        }
    }

    pub fn set_route(&mut self, route: ModelRoute) {
        self.provider = route.connection.to_string();
        self.model = route.model;
    }

    pub fn canonical_model_spec(&self) -> String {
        let route = self.route();
        if route.model.is_empty() {
            return String::new();
        }
        route.to_string()
    }
}
