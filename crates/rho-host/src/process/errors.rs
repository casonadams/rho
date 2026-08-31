use rho_sdk::protocol::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    #[error("failed to start plugin process")]
    Spawn,
    #[error("plugin process I/O failed")]
    Io,
    #[error("plugin process returned malformed protocol data")]
    MalformedProtocol,
    #[error("plugin process returned an oversized message")]
    OversizedMessage,
    #[error("plugin process closed its output unexpectedly")]
    UnexpectedEof,
    #[error("plugin process returned an unsupported protocol version")]
    UnsupportedVersion,
    #[error("plugin response correlation failed")]
    CorrelationMismatch,
    #[error("plugin process returned an unexpected response")]
    UnexpectedResponse,
    #[error("plugin manifest is invalid")]
    InvalidManifest,
    #[error("plugin capability declaration is invalid")]
    InvalidCapability,
    #[error("plugin startup timed out")]
    StartupTimeout,
    #[error("plugin discovery timed out")]
    DiscoveryTimeout,
    #[error("plugin invocation timed out")]
    InvocationTimeout,
    #[error("plugin cancellation timed out")]
    CancellationTimeout,
    #[error("plugin returned {code:?}; retryable: {retryable}")]
    Remote { code: ErrorCode, retryable: bool },
    #[error("plugin process task failed")]
    ProcessTaskFailed,
    #[error("{0}; plugin stderr diagnostic was redacted")]
    FailureWithRedactedStderr(String),
}
