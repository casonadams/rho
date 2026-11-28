pub mod accumulator;
pub mod direnv;
pub mod failures;
pub mod footer;
mod log;
pub mod normalize;
mod poll;
pub mod runner;
pub mod sanitize;
pub mod shell;
use crate::tools::WorkspaceDir;
use crate::tools::engine_tool::{EngineTool, TurnToolContext, TypedTool};
use crate::tools::types::ToolResult;
pub use accumulator::{OutputAccumulator, OutputSnapshot};
pub(crate) use direnv::DirenvOptions;
pub use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::presentation::ToolStreamPort;
pub use runner::DEFAULT_BASH_TIMEOUT_SEC;
pub(crate) use runner::run_command_streaming_with_direnv;
pub use sanitize::{sanitize_binary_output, split_at_incomplete_ansi};
pub(crate) use shell::resolve_shell_command;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub struct BashTool {
    pub base_dir: WorkspaceDir,
    log_dir: PathBuf,
    /// The agent whose mailbox hears about commands this tool moves to the background.
    owner: Option<crate::mailbox::Mailbox>,
    direnv: DirenvOptions,
    /// Where the output of a running command goes as it is produced.
    progress: ToolStreamPort,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: WorkspaceDir::new(base_dir.as_ref()),
            log_dir: accumulator::default_log_dir(),
            owner: None,
            direnv: DirenvOptions::default(),
            progress: ToolStreamPort::default(),
        }
    }

    #[must_use]
    pub(crate) fn with_workspace_dir(mut self, base_dir: WorkspaceDir) -> Self {
        self.base_dir = base_dir;
        self
    }

    /// Writes full output logs under `dir` instead of the shared temp-dir cache.
    #[must_use]
    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = dir.into();
        self
    }

    /// Configures automatic `direnv` environment loading for commands.
    #[must_use]
    pub(crate) fn with_direnv(mut self, direnv: DirenvOptions) -> Self {
        self.direnv = direnv;
        self
    }

    pub async fn execute_streaming<F>(&self, args: BashArgs, on_chunk: F) -> Result<ToolResult, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let base_dir = self.base_dir.get();
        run_command_streaming_with_direnv(
            &base_dir,
            &self.log_dir,
            self.owner.as_ref(),
            &args,
            self.direnv,
            on_chunk,
        )
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
        if let Some(refusal) = poll::refusal(self.owner.as_ref(), &args) {
            return Ok(refusal);
        }
        let port = self.progress.clone();
        self.execute_streaming(args, move |chunk| port.stream_chunk(chunk))
            .await
    }

    #[cfg(test)]
    fn direnv_options(&self) -> Option<DirenvOptions> {
        Some(self.direnv)
    }

    fn bind_to_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(Self {
            base_dir: self.base_dir.clone(),
            log_dir: self.log_dir.clone(),
            owner: ctx.inbox.clone(),
            direnv: self.direnv,
            progress: ctx.progress.clone(),
        }))
    }
}
