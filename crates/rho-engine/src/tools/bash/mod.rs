pub mod accumulator;
pub mod normalize;
pub mod runner;
pub mod sanitize;
pub mod shell;

use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::{ToolResult, generated_schema};
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
    /// The agent whose mailbox hears about commands this tool moves to the background.
    owner: Option<crate::mailbox::Mailbox>,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            owner: None,
        }
    }

    pub async fn execute_streaming<F>(&self, args: BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        run_command_streaming(&self.base_dir, self.owner.as_ref(), &args, on_chunk)
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
impl EngineTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        crate::tools::ToolRegistry::builtin_description("bash")
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<BashArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: BashArgs = match crate::tools::types::parse_tool_args("bash", args, || self.parameters()) {
            Ok(a) => a,
            Err(e) => return Ok(e),
        };
        self.execute(args).await
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        let owner = ctx.inbox.clone()?;
        Some(Arc::new(Self {
            base_dir: self.base_dir.clone(),
            owner: Some(owner),
        }))
    }
}
