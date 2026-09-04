mod entry;
mod query;
mod stats;
#[cfg(test)]
mod tests;

pub use entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, format_results, sort_entries};
use query::FdQuery;
pub use rho_harness_core::args::{FdArgs, FdSort};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};

use crate::tools::traversal::{build_type_matcher, search_root};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use regex::Regex;

pub const DEFAULT_FD_LIMIT: usize = 200;
pub const MAX_FD_LIMIT: usize = 1000;
pub const MAX_FD_DEPTH: usize = 10;

pub struct FdTool {
    base_dir: PathBuf,
}

impl FdTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: FdArgs) -> Result<ToolResult, AppError> {
        let pattern = args.pattern.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let regex = match pattern {
            Some(p) => match compile_pattern(p) {
                Ok(regex) => Some(regex),
                Err(message) => return Ok(ToolResult::error(message)),
            },
            None => None,
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
        let limit = args.limit.unwrap_or(DEFAULT_FD_LIMIT).clamp(1, MAX_FD_LIMIT);
        let show_stats = args.stats.unwrap_or(
            args.min_lines.is_some()
                || args.max_lines.is_some()
                || matches!(args.sort, Some(FdSort::Lines | FdSort::Size)),
        );
        let stats_needed = show_stats
            || args.min_lines.is_some()
            || args.max_lines.is_some()
            || matches!(args.sort, Some(FdSort::Lines | FdSort::Size));

        let query = FdQuery {
            workspace_root: workspace.root().to_path_buf(),
            search_root,
            regex,
            types,
            include_hidden: args.hidden.unwrap_or(false),
            depth: args.depth.map(|depth| depth.clamp(1, MAX_FD_DEPTH)),
            stats_needed,
            min_lines: args.min_lines,
            max_lines: args.max_lines,
            sort: args.sort,
            show_stats,
        };
        match tokio::task::spawn_blocking(move || query.run(limit)).await {
            Ok(result) => Ok(result),
            Err(error) => Err(AppError::Tool(format!("fd traversal task failed: {error}"))),
        }
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

impl Tool for FdTool {
    const NAME: &'static str = "fd";
    type Args = FdArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Find files and directories by workspace-relative path with a smart-case regex; gitignore-aware and bounded."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<FdArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
