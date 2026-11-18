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
    #[error("The provider filtered the model response")]
    ContentFiltered,
    #[error("Operation cancelled: {0}")]
    Cancelled(String),
    #[error("MCP error: {0}")]
    Mcp(String),
    #[error("MCP connection lost: {0}")]
    McpConnectionLost(String),
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

    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Network(_) | Self::RateLimited { .. } | Self::ProviderUnavailable { .. }
        )
    }
}
