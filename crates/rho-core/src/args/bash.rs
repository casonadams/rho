use serde::{Deserialize, Serialize};

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BashArgs {
    /// Command to execute
    pub command: String,
    /// Timeout in seconds (default: 30)
    pub timeout: Option<u64>,
}
