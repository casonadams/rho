pub mod accumulator;
pub mod failures;
pub mod footer;
mod log;
pub mod normalize;
mod poll;
pub mod runner;
pub mod sanitize;
pub mod shell;

use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::types::ToolResult;
pub use accumulator::{OutputAccumulator, OutputSnapshot};
pub use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
pub use runner::{DEFAULT_BASH_TIMEOUT_SEC, run_command_streaming};
pub use sanitize::{sanitize_binary_output, split_at_incomplete_ansi};
pub use shell::resolve_shell_command;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub struct BashTool {
    pub base_dir: PathBuf,
    log_dir: PathBuf,
    /// The agent whose mailbox hears about commands this tool moves to the background.
    owner: Option<crate::mailbox::Mailbox>,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            log_dir: accumulator::default_log_dir(),
            owner: None,
        }
    }

    /// Writes full output logs under `dir` instead of the shared temp-dir cache.
    #[must_use]
    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = dir.into();
        self
    }

    pub async fn execute_streaming<F>(&self, args: BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        run_command_streaming(&self.base_dir, &self.log_dir, self.owner.as_ref(), &args, on_chunk)
            .await
            .map(plain_text_for_model)
    }

    pub async fn execute(&self, args: BashArgs) -> Result<ToolResult, AppError> {
        self.execute_streaming(args, |_| {}).await
    }
}

/// Commands run with color forced on so the terminal shows it; the model gets the same text
/// without escape sequences, and the colored original stays available as the display text.
fn plain_text_for_model(result: ToolResult) -> ToolResult {
    if !result.content.contains('\u{1b}') {
        return result;
    }
    ToolResult {
        content: sanitize::strip_ansi(&result.content).into_owned(),
        display: Some(result.content),
        ..result
    }
}

#[async_trait::async_trait]
impl TypedTool for BashTool {
    type Args = BashArgs;
    const NAME: &'static str = "bash";

    async fn run(&self, args: BashArgs) -> Result<ToolResult, AppError> {
        if let Some(refusal) = poll::refusal(self.owner.as_ref(), &args.command) {
            return Ok(refusal);
        }
        self.execute(args).await
    }

    fn bind_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        let owner = ctx.inbox.clone()?;
        Some(Arc::new(Self {
            base_dir: self.base_dir.clone(),
            log_dir: self.log_dir.clone(),
            owner: Some(owner),
        }))
    }
}
