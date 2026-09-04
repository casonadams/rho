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
        }
    }
}
