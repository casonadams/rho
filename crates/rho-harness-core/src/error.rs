pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Policy violation: {0}")]
    Policy(String),
    #[error("Tool execution error: {0}")]
    Tool(String),
    #[error("Session error: {0}")]
    Session(String),
    #[error("Model-call budget exhausted after {max_turns} calls")]
    ModelBudgetExhausted { max_turns: usize },
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Rate limited: {message}")]
    RateLimited {
        message: String,
        retry_after: Option<std::time::Duration>,
    },
    #[error("Provider unavailable: {message}")]
    ProviderUnavailable {
        message: String,
        retry_after: Option<std::time::Duration>,
    },
    #[error("Context window exceeded: {0}")]
    ContextOverflow(String),
    #[error("Context reached {used} tokens, past where compaction starts")]
    CompactionDue { used: u64 },
    #[error("The provider filtered the model response")]
    ContentFiltered,
    #[error("Operation cancelled: {0}")]
    Cancelled(String),
    #[error("MCP error: {0}")]
    Mcp(String),
    #[error("MCP connection lost: {0}")]
    McpConnectionLost(String),
    #[error("MCP request not delivered: {0}")]
    McpNotDelivered(String),
    #[error("Hook error: {0}")]
    Hook(String),
    #[error("Invalid tool call: {0}")]
    InvalidToolCall(String),
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::RateLimited { retry_after, .. } | Self::ProviderUnavailable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// The turn stopped because its context is full or nearly so, and continues once compacted.
    pub fn needs_compaction(&self) -> bool {
        matches!(self, Self::ContextOverflow(_) | Self::CompactionDue { .. })
    }

    /// The turn stopped before an overflow, so it can also go on uncompacted.
    pub fn is_early_stop(&self) -> bool {
        matches!(self, Self::CompactionDue { .. })
    }

    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Network(_) | Self::RateLimited { .. } | Self::ProviderUnavailable { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use test_case::test_case;

    #[test_case(AppError::CompactionDue { used: 1 }, true, true; "compaction due")]
    #[test_case(AppError::ContextOverflow("too long".into()), true, false; "context overflow")]
    #[test_case(AppError::Network("reset".into()), false, false; "network error")]
    fn only_a_full_context_needs_compaction_and_only_an_early_stop_can_run_on(
        error: AppError,
        needs_compaction: bool,
        early_stop: bool,
    ) {
        assert_eq!(error.needs_compaction(), needs_compaction);
        assert_eq!(error.is_early_stop(), early_stop);
    }
}
