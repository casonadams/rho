use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub(crate) fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransportKind {
    #[default]
    Stdio,
    StreamableHttp,
    Sse,
}

pub fn default_idle_timeout_seconds() -> u64 {
    600
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum McpLifecycleMode {
    #[default]
    Lazy,
    Eager,
    KeepAlive,
    LazyKeepAlive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpDirectTools {
    All(bool),
    Search(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum McpExposureMode {
    #[default]
    Auto,
    Direct,
    Gateway,
}

pub fn default_defer_threshold() -> usize {
    4
}

pub fn default_startup_timeout_ms() -> u64 {
    3000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_defer_threshold", alias = "deferThreshold")]
    pub defer_threshold: usize,
    #[serde(default = "default_idle_timeout_seconds", alias = "idleTimeout")]
    pub idle_timeout_seconds: u64,
    #[serde(default = "default_startup_timeout_ms", alias = "startupTimeoutMs")]
    pub startup_timeout_ms: u64,
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
    #[serde(skip)]
    pub project: Option<ProjectMcpServers>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMcpServers {
    pub workspace: PathBuf,
    pub names: BTreeSet<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            defer_threshold: 4,
            idle_timeout_seconds: default_idle_timeout_seconds(),
            startup_timeout_ms: default_startup_timeout_ms(),
            servers: BTreeMap::new(),
            project: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransportKind>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<McpExposureMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<McpLifecycleMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_tools: Option<McpDirectTools>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub search_keywords: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct McpOAuthConfig {
    #[serde(default, alias = "clientId", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, alias = "clientSecret", skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, alias = "redirectPort", skip_serializing_if = "Option::is_none")]
    pub redirect_port: Option<u16>,
    #[serde(default, alias = "redirectUri", skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    #[serde(default, alias = "authorizationParams", skip_serializing_if = "BTreeMap::is_empty")]
    pub authorization_params: BTreeMap<String, String>,
}

impl McpServerConfig {
    pub fn stdio(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: Some(command.into()),
            args,
            enabled: true,
            transport: Some(McpTransportKind::Stdio),
            ..Default::default()
        }
    }

    pub fn is_remote(&self) -> bool {
        self.url.is_some()
    }

    pub fn uses_oauth(&self) -> bool {
        self.url.is_some() && (self.oauth.is_some() || self.headers.is_empty())
    }

    pub fn resolved_transport(&self) -> McpTransportKind {
        if let Some(t) = self.transport {
            t
        } else if self.url.is_some() {
            McpTransportKind::StreamableHttp
        } else {
            McpTransportKind::Stdio
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}
