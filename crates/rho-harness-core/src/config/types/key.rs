use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigKey {
    Model,
    Provider,
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
    SessionRetentionDays,
    BlockStyle,
    AgentBlockOutput,
    HideThinking,
    ToolsExpanded,
    Cursor,
    ShowLabel,
    SearchEngine,
    WebSearchEnabled,
    WebFetchEnabled,
    McpEnabled,
    PermissionEnabled,
    GuardModel,
    SmolModel,
    SlowModel,
}

const KEY_NAMES: &[(ConfigKey, &str, &[&str])] = &[
    (ConfigKey::Model, "model", &["default_model", "models.default"]),
    (ConfigKey::Provider, "provider", &["default_provider", "model_provider"]),
    (ConfigKey::MaxOutputTokens, "max_output_tokens", &[]),
    (ConfigKey::MaxTurns, "max_turns", &[]),
    (ConfigKey::ContextLimit, "context_limit", &[]),
    (ConfigKey::ContextWindowMessages, "context_window_messages", &[]),
    (ConfigKey::CompactionMaxBytes, "compaction_max_bytes", &[]),
    (ConfigKey::SearchMinIntervalMs, "search_min_interval_ms", &[]),
    (ConfigKey::SearchTimeoutSec, "search_timeout_sec", &[]),
    (ConfigKey::FetchTimeoutSec, "fetch_timeout_sec", &[]),
    (ConfigKey::FetchLimit, "fetch_limit", &[]),
    (ConfigKey::FetchMaxBytes, "fetch_max_bytes", &[]),
    (ConfigKey::OutputMaxBytes, "output_max_bytes", &[]),
    (ConfigKey::AllowPrivateNetwork, "allow_private_network", &[]),
    (ConfigKey::Region, "region", &[]),
    (ConfigKey::SteeringMode, "steering_mode", &[]),
    (ConfigKey::FollowUpMode, "follow_up_mode", &[]),
    (ConfigKey::ReserveTokens, "reserve_tokens", &[]),
    (ConfigKey::KeepRecentTokens, "keep_recent_tokens", &[]),
    (
        ConfigKey::ThinkingLevel,
        "thinking_level",
        &["thinking", "default_thinking", "default_thinking_level"],
    ),
    (
        ConfigKey::SessionRetentionDays,
        "session_retention_days",
        &["retention_days"],
    ),
    (ConfigKey::BlockStyle, "ui.block_style", &["block_style"]),
    (
        ConfigKey::AgentBlockOutput,
        "ui.agent_block_output",
        &["agent_block_output", "agent_box", "ui.agent_box"],
    ),
    (
        ConfigKey::HideThinking,
        "ui.hide_thinking",
        &["hide_thinking", "thinking_hidden", "ui.thinking_hidden"],
    ),
    (
        ConfigKey::ToolsExpanded,
        "ui.tools_expanded",
        &["tools_expanded", "expand_tools", "ui.expand_tools"],
    ),
    (
        ConfigKey::Cursor,
        "ui.cursor",
        &[
            "cursor",
            "cursor_style",
            "ui.cursor_style",
            "cursor_mode",
            "ui.cursor_mode",
        ],
    ),
    (ConfigKey::ShowLabel, "show_label", &[]),
    (
        ConfigKey::SearchEngine,
        "tools.web.search.default",
        &["tools.web.search", "search_engine", "search_provider"],
    ),
    (ConfigKey::WebSearchEnabled, "tools.web.search.enabled", &["web_search"]),
    (ConfigKey::WebFetchEnabled, "tools.web.fetch.enabled", &["web_fetch"]),
    (ConfigKey::McpEnabled, "mcp.enabled", &["mcp"]),
    (ConfigKey::PermissionEnabled, "permission.enabled", &["permission"]),
    (ConfigKey::GuardModel, "models.guard", &["guard_model", "guard"]),
    (ConfigKey::SmolModel, "models.smol", &["smol_model", "smol"]),
    (ConfigKey::SlowModel, "models.slow", &["slow_model", "slow"]),
];

impl FromStr for ConfigKey {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        KEY_NAMES
            .iter()
            .find(|(_, name, aliases)| *name == value || aliases.contains(&value))
            .map(|(key, _, _)| *key)
            .ok_or_else(|| format!("unknown configuration key: {value}"))
    }
}

impl ConfigKey {
    pub(crate) fn as_str(self) -> &'static str {
        KEY_NAMES[self as usize].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYS: [ConfigKey; 35] = [
        ConfigKey::Model,
        ConfigKey::Provider,
        ConfigKey::MaxOutputTokens,
        ConfigKey::MaxTurns,
        ConfigKey::ContextLimit,
        ConfigKey::ContextWindowMessages,
        ConfigKey::CompactionMaxBytes,
        ConfigKey::SearchMinIntervalMs,
        ConfigKey::SearchTimeoutSec,
        ConfigKey::FetchTimeoutSec,
        ConfigKey::FetchLimit,
        ConfigKey::FetchMaxBytes,
        ConfigKey::OutputMaxBytes,
        ConfigKey::AllowPrivateNetwork,
        ConfigKey::Region,
        ConfigKey::SteeringMode,
        ConfigKey::FollowUpMode,
        ConfigKey::ReserveTokens,
        ConfigKey::KeepRecentTokens,
        ConfigKey::ThinkingLevel,
        ConfigKey::SessionRetentionDays,
        ConfigKey::BlockStyle,
        ConfigKey::AgentBlockOutput,
        ConfigKey::HideThinking,
        ConfigKey::ToolsExpanded,
        ConfigKey::Cursor,
        ConfigKey::ShowLabel,
        ConfigKey::SearchEngine,
        ConfigKey::WebSearchEnabled,
        ConfigKey::WebFetchEnabled,
        ConfigKey::McpEnabled,
        ConfigKey::PermissionEnabled,
        ConfigKey::GuardModel,
        ConfigKey::SmolModel,
        ConfigKey::SlowModel,
    ];

    #[test]
    fn all_keys_roundtrip_through_str() {
        for key in ALL_KEYS {
            let s = key.as_str();
            assert!(!s.is_empty());
            assert_eq!(ConfigKey::from_str(s), Ok(key));
        }
    }

    #[test]
    fn key_aliases_parse_correctly() {
        let aliases = [
            ("default_model", ConfigKey::Model),
            ("default_provider", ConfigKey::Provider),
            ("model_provider", ConfigKey::Provider),
            ("thinking", ConfigKey::ThinkingLevel),
            ("default_thinking", ConfigKey::ThinkingLevel),
            ("default_thinking_level", ConfigKey::ThinkingLevel),
            ("retention_days", ConfigKey::SessionRetentionDays),
            ("agent_box", ConfigKey::AgentBlockOutput),
            ("ui.agent_box", ConfigKey::AgentBlockOutput),
            ("thinking_hidden", ConfigKey::HideThinking),
            ("ui.thinking_hidden", ConfigKey::HideThinking),
            ("expand_tools", ConfigKey::ToolsExpanded),
            ("ui.expand_tools", ConfigKey::ToolsExpanded),
            ("cursor_style", ConfigKey::Cursor),
            ("ui.cursor_style", ConfigKey::Cursor),
            ("cursor_mode", ConfigKey::Cursor),
            ("ui.cursor_mode", ConfigKey::Cursor),
            ("tools.web.search", ConfigKey::SearchEngine),
            ("search_engine", ConfigKey::SearchEngine),
            ("search_provider", ConfigKey::SearchEngine),
            ("web_search", ConfigKey::WebSearchEnabled),
            ("web_fetch", ConfigKey::WebFetchEnabled),
            ("mcp", ConfigKey::McpEnabled),
            ("permission", ConfigKey::PermissionEnabled),
            ("guard", ConfigKey::GuardModel),
            ("guard_model", ConfigKey::GuardModel),
        ];
        for (alias, expected) in aliases {
            assert_eq!(ConfigKey::from_str(alias), Ok(expected));
        }
    }

    #[test]
    fn unknown_key_fails() {
        assert!(ConfigKey::from_str("completely_unknown_setting").is_err());
    }

    #[test]
    fn names_are_indexed_by_discriminant() {
        assert_eq!(KEY_NAMES.len(), ALL_KEYS.len());
        for (index, key) in ALL_KEYS.into_iter().enumerate() {
            assert_eq!(key as usize, index);
            assert_eq!(key.as_str(), KEY_NAMES[index].1);
        }
    }
}
