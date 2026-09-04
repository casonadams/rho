mod entry;
mod query;
#[cfg(test)]
mod tests;

pub use entry::{LineMatch, RG_COLLECTION_CEILING, format_results, render};
pub use query::{MAX_RG_FILE_BYTES, RgQuery};
pub use rho_harness_core::args::RgArgs;

use crate::tools::traversal::{build_type_matcher, search_root};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

pub const DEFAULT_RG_LIMIT: usize = 200;
pub const MAX_RG_LIMIT: usize = 1000;

pub struct RgTool {
    base_dir: PathBuf,
}

impl RgTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: RgArgs) -> Result<ToolResult, AppError> {
        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Ok(ToolResult::error("Empty pattern provided for rg tool"));
        }
        let matcher = match compile_matcher(pattern) {
            Ok(matcher) => matcher,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let types = match build_type_matcher(args.file_type.as_deref()) {
            Ok(types) => types,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let workspace = Workspace::new(&self.base_dir);
        let search_root = match search_root(&workspace, args.path.as_deref()) {
            Ok(root) => root,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let limit = args.limit.unwrap_or(DEFAULT_RG_LIMIT).clamp(1, MAX_RG_LIMIT);
        let query = RgQuery {
            workspace_root: workspace.root().to_path_buf(),
            search_root,
            matcher,
            types,
            include_hidden: args.hidden.unwrap_or(false),
        };
        match tokio::task::spawn_blocking(move || query.run(limit)).await {
            Ok(result) => Ok(result),
            Err(error) => Err(AppError::Tool(format!("rg search task failed: {error}"))),
        }
    }
}

fn compile_matcher(pattern: &str) -> Result<RegexMatcher, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    RegexMatcherBuilder::new()
        .case_insensitive(case_insensitive)
        .build(pattern)
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

impl Tool for RgTool {
    const NAME: &'static str = "rg";
    type Args = RgArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Search file contents with a smart-case regex; gitignore-aware, skips binary and large files, bounded."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<RgArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
