mod app;
mod file;
mod integrations;
mod key;
mod paths;
mod ui;

pub use app::{
    ApprovalFocus, BashConfig, CacheRetention, Config, DEFAULT_MAX_TURNS, LspConfig, ModelRole, SubagentsConfig,
    ToolsConfig, WebFetchConfig, WebSearchConfig, WebToolsConfig, WorktreeConfig,
};
pub use file::ModelsConfig;
pub(crate) use file::{BashConfigFile, FileConfig, LspConfigFile, WebFetchConfigFile, WebSearchConfigFile};
pub use integrations::{
    McpConfig, McpDirectTools, McpExposureMode, McpLifecycleMode, McpOAuthConfig, McpServerConfig, McpTransportKind,
    PermissionConfig, ProjectMcpServers, ProviderConfig, default_idle_timeout_seconds,
};
pub(crate) use key::ConfigKey;
pub use paths::{config_dir, default_config_dir, dirs_fallback, home_dir};
pub use ui::{
    A2uiConfig, ContextLineMode, DisplayUiConfig, DividerStyle, GitSegmentOptions, ModelSegmentOptions,
    PathSegmentOptions, PromptUiConfig, StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId,
    StatusBarSegmentOptions, StatusBarSeparator, TimeFormat, TimeSegmentOptions, UiConfig,
};
