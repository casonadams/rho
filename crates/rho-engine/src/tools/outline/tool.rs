use std::path::{Path, PathBuf};

use rho_harness_core::args::OutlineArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};

use super::search::{OutlineSearchOptions, search_outline};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};

pub struct OutlineTool {
    base_dir: PathBuf,
}

impl OutlineTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: OutlineArgs) -> Result<ToolResult, AppError> {
        let workspace = Workspace::new(&self.base_dir);
        let path = args.path;
        let query = args.query;
        let kind = args.kind;
        let depth = args.depth;

        let result = tokio::task::spawn_blocking(move || {
            search_outline(
                &workspace,
                OutlineSearchOptions {
                    path: &path,
                    query: query.as_deref(),
                    kind: kind.as_deref(),
                    depth,
                },
            )
        })
        .await
        .map_err(|e| AppError::Tool(format!("outline task failed: {e}")))?;

        match result {
            Ok(tool_result) => Ok(tool_result),
            Err(err_msg) => Ok(ToolResult::error(err_msg)),
        }
    }
}

impl Tool for OutlineTool {
    const NAME: &'static str = "outline";
    type Args = OutlineArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Extract syntax-aware symbol outlines (functions, methods, classes, structs, traits) without implementation bodies."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<OutlineArgs>()
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
