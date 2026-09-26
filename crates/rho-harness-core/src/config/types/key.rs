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
    SemanticSearch,
    SearchEngine,
    WebSearchEnabled,
    WebFetchEnabled,
    WebFetchMultimodal,
    McpEnabled,
    PermissionEnabled,
    GuardModel,
}

impl FromStr for ConfigKey {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(key) = parse_runtime_key(value) {
            return Ok(key);
        }
        if let Some(key) = parse_feature_key(value) {
            return Ok(key);
        }
        Err(format!("unknown configuration key: {value}"))
    }
}

fn parse_runtime_key(value: &str) -> Option<ConfigKey> {
    match value {
        "model" | "default_model" => Some(ConfigKey::Model),
        "provider" | "default_provider" | "model_provider" => Some(ConfigKey::Provider),
        "thinking_level" | "thinking" | "default_thinking" | "default_thinking_level" => Some(ConfigKey::ThinkingLevel),
        "max_output_tokens" => Some(ConfigKey::MaxOutputTokens),
        "max_turns" => Some(ConfigKey::MaxTurns),
        "context_limit" => Some(ConfigKey::ContextLimit),
        "context_window_messages" => Some(ConfigKey::ContextWindowMessages),
        "compaction_max_bytes" => Some(ConfigKey::CompactionMaxBytes),
        "search_min_interval_ms" => Some(ConfigKey::SearchMinIntervalMs),
        "search_timeout_sec" => Some(ConfigKey::SearchTimeoutSec),
        "fetch_timeout_sec" => Some(ConfigKey::FetchTimeoutSec),
        "fetch_limit" => Some(ConfigKey::FetchLimit),
        "fetch_max_bytes" => Some(ConfigKey::FetchMaxBytes),
        "output_max_bytes" => Some(ConfigKey::OutputMaxBytes),
        "allow_private_network" => Some(ConfigKey::AllowPrivateNetwork),
        "region" => Some(ConfigKey::Region),
        "steering_mode" => Some(ConfigKey::SteeringMode),
        "follow_up_mode" => Some(ConfigKey::FollowUpMode),
        "reserve_tokens" => Some(ConfigKey::ReserveTokens),
        "keep_recent_tokens" => Some(ConfigKey::KeepRecentTokens),
        "session_retention_days" | "retention_days" => Some(ConfigKey::SessionRetentionDays),
        _ => None,
    }
}

fn parse_feature_key(value: &str) -> Option<ConfigKey> {
    match value {
        "block_style" | "ui.block_style" => Some(ConfigKey::BlockStyle),
        "agent_block_output" | "ui.agent_block_output" | "agent_box" | "ui.agent_box" => {
            Some(ConfigKey::AgentBlockOutput)
        }
        "hide_thinking" | "ui.hide_thinking" | "thinking_hidden" | "ui.thinking_hidden" => {
            Some(ConfigKey::HideThinking)
        }
        "tools_expanded" | "ui.tools_expanded" | "expand_tools" | "ui.expand_tools" => Some(ConfigKey::ToolsExpanded),
        "cursor" | "ui.cursor" | "cursor_style" | "ui.cursor_style" | "cursor_mode" | "ui.cursor_mode" => {
            Some(ConfigKey::Cursor)
        }
        "show_label" => Some(ConfigKey::ShowLabel),
        "semantic_search" | "semantic-search" | "features.semantic_search" => Some(ConfigKey::SemanticSearch),
        "tools.web.search.default" | "tools.web.search" | "search_engine" | "search_provider" => {
            Some(ConfigKey::SearchEngine)
        }
        "tools.web.search.enabled" | "web_search" => Some(ConfigKey::WebSearchEnabled),
        "tools.web.fetch.enabled" | "web_fetch" => Some(ConfigKey::WebFetchEnabled),
        "tools.web.fetch.multimodal" | "web_fetch_multimodal" => Some(ConfigKey::WebFetchMultimodal),
        "mcp.enabled" | "mcp" => Some(ConfigKey::McpEnabled),
        "permission.enabled" | "permission" => Some(ConfigKey::PermissionEnabled),
        "models.guard" | "guard_model" | "guard" => Some(ConfigKey::GuardModel),
        _ => None,
    }
}

impl ConfigKey {
    pub(crate) const fn as_str(self) -> &'static str {
        if let Some(name) = self.runtime_key_name() {
            name
        } else {
            self.feature_key_name()
        }
    }

    const fn runtime_key_name(self) -> Option<&'static str> {
        match self {
            Self::Model => Some("model"),
            Self::Provider => Some("provider"),
            Self::MaxOutputTokens => Some("max_output_tokens"),
            Self::MaxTurns => Some("max_turns"),
            Self::ContextLimit => Some("context_limit"),
            Self::ContextWindowMessages => Some("context_window_messages"),
            Self::CompactionMaxBytes => Some("compaction_max_bytes"),
            Self::SearchMinIntervalMs => Some("search_min_interval_ms"),
            Self::SearchTimeoutSec => Some("search_timeout_sec"),
            Self::FetchTimeoutSec => Some("fetch_timeout_sec"),
            Self::FetchLimit => Some("fetch_limit"),
            Self::FetchMaxBytes => Some("fetch_max_bytes"),
            Self::OutputMaxBytes => Some("output_max_bytes"),
            Self::AllowPrivateNetwork => Some("allow_private_network"),
            Self::Region => Some("region"),
            Self::SteeringMode => Some("steering_mode"),
            Self::FollowUpMode => Some("follow_up_mode"),
            Self::ReserveTokens => Some("reserve_tokens"),
            Self::KeepRecentTokens => Some("keep_recent_tokens"),
            Self::ThinkingLevel => Some("thinking_level"),
            Self::SessionRetentionDays => Some("session_retention_days"),
            _ => None,
        }
    }

    const fn feature_key_name(self) -> &'static str {
        match self {
            Self::BlockStyle => "ui.block_style",
            Self::AgentBlockOutput => "ui.agent_block_output",
            Self::HideThinking => "ui.hide_thinking",
            Self::ToolsExpanded => "ui.tools_expanded",
            Self::Cursor => "ui.cursor",
            Self::ShowLabel => "show_label",
            Self::SemanticSearch => "semantic_search",
            Self::SearchEngine => "tools.web.search.default",
            Self::WebSearchEnabled => "tools.web.search.enabled",
            Self::WebFetchEnabled => "tools.web.fetch.enabled",
            Self::WebFetchMultimodal => "tools.web.fetch.multimodal",
            Self::McpEnabled => "mcp.enabled",
            Self::PermissionEnabled => "permission.enabled",
            Self::GuardModel => "models.guard",
            _ => "",
        }
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
        ConfigKey::SemanticSearch,
        ConfigKey::SearchEngine,
        ConfigKey::WebSearchEnabled,
        ConfigKey::WebFetchEnabled,
        ConfigKey::WebFetchMultimodal,
        ConfigKey::McpEnabled,
        ConfigKey::PermissionEnabled,
        ConfigKey::GuardModel,
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
            ("semantic-search", ConfigKey::SemanticSearch),
            ("features.semantic_search", ConfigKey::SemanticSearch),
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
    fn feature_key_name_fallback_for_runtime_keys() {
        assert_eq!(ConfigKey::Model.feature_key_name(), "");
    }
}
