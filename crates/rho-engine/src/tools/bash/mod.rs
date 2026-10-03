pub mod accumulator;
pub mod normalize;
pub mod read_only;
pub mod runner;
pub mod sanitize;
pub mod shell;

use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
pub use accumulator::{OutputAccumulator, OutputSnapshot};
pub use read_only::is_read_only_command;
pub use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
pub use runner::{DEFAULT_BASH_TIMEOUT_SEC, run_command_streaming};
pub use sanitize::{sanitize_binary_output, split_at_incomplete_ansi};
pub use shell::resolve_shell_command;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub struct BashTool {
    pub base_dir: PathBuf,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute_streaming<F>(&self, args: BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        run_command_streaming(&self.base_dir, &args, on_chunk).await
    }

    pub async fn execute(&self, args: BashArgs) -> Result<ToolResult, AppError> {
        self.execute_streaming(args, |_| {}).await
    }
}

#[async_trait::async_trait]
impl EngineTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd."
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<BashArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: BashArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse tool arguments: {e}"))),
        };
        self.execute(args).await
    }
}
