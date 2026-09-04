pub mod accumulator;
pub mod read_only;
pub mod runner;
pub mod sanitize;
pub mod shell;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use accumulator::{OutputAccumulator, OutputSnapshot};
pub use read_only::is_read_only_command;
pub use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
pub use runner::{DEFAULT_BASH_TIMEOUT_SEC, run_command_streaming};
pub use sanitize::sanitize_binary_output;
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

impl Tool for BashTool {
    const NAME: &'static str = "bash";
    type Args = BashArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<BashArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
